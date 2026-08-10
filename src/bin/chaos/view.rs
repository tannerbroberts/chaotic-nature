//! Drawing. Everything here is presentation — no value computed in this file
//! ever reaches the simulation.

// Presentation only. The simulation this renders never sees a float.
#![allow(clippy::float_arithmetic)]

use std::io::Write;

use pentagram::element::{Element, PerElement};
use pentagram::race::RateBand;
use pentagram::world::{Event, World};

use crate::knobs::{abbrev, duration, grouped, Tuning, PAGES};

/// Which screen the player is on. The whole soul → body → soul loop is a walk
/// around this enum.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// The instrument: knob table and governor readouts.
    Tune,
    /// Disembodied. The whole map, the open incarnation events, and a choice.
    Soul,
    /// Incarnated as this entity id, until death or release.
    Body(u32),
}

const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Population samples kept. One per frame at 20 fps, so about a minute of wall
/// clock — long enough to see a boom and the bust that follows it.
pub const HISTORY: usize = 48;

/// Element colours, matching the design document.
const RGB: PerElement<(u8, u8, u8)> = PerElement([
    (127, 176, 105), // Wood
    (226, 105, 74),  // Fire
    (211, 164, 69),  // Earth
    (173, 164, 206), // Metal
    (89, 160, 198),  // Water
]);

const DIM: &str = "\x1b[38;2;110;118;134m";
const BRIGHT: &str = "\x1b[38;2;232;234;239m";
const INVERT: &str = "\x1b[7m";
const RESET: &str = "\x1b[0m";
const CLEAR_EOL: &str = "\x1b[K";

fn col(c: (u8, u8, u8)) -> String {
    format!("\x1b[38;2;{};{};{}m", c.0, c.1, c.2)
}

/// Where the cursor is and what is switched on. The only mutable UI state.
pub struct View {
    pub mode: Mode,
    pub page: usize,
    pub row: usize,
    pub col: usize,
    /// Selection into the soul view's event list (sorted by closing time).
    pub sel: usize,
    pub show_map: bool,
    pub history: PerElement<Vec<u32>>,
    /// Transient message shown in the footer, with the frames it has left.
    pub notice: Option<(String, u32)>,
}

impl View {
    pub fn new() -> View {
        View {
            // A player arrives as a soul; the knob table is one keypress away.
            mode: Mode::Soul,
            page: 0,
            row: 0,
            col: 0,
            sel: 0,
            show_map: true,
            history: PerElement([vec![], vec![], vec![], vec![], vec![]]),
            notice: None,
        }
    }

    /// The soul view's event list: soonest-closing first, id as tiebreak, so
    /// the moment about to pass is always at the top.
    pub fn sorted_events(w: &World) -> Vec<Event> {
        let mut evs = w.events.clone();
        evs.sort_by_key(|e| (e.closes, e.id));
        evs
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), 60));
    }

    pub fn element(&self) -> Element {
        Element::from_index(self.col)
    }

    pub fn knobs(&self) -> &'static [crate::knobs::Knob] {
        PAGES[self.page].knobs
    }

    pub fn pages() -> usize {
        PAGES.len()
    }

    pub fn sample(&mut self, w: &World) {
        let pop = w.population();
        for e in Element::ALL {
            let h = self.history.get_mut(e);
            h.push(pop[e]);
            if h.len() > HISTORY {
                h.remove(0);
            }
        }
    }

    /// Keep the cursor on a real cell after a page change.
    pub fn clamp(&mut self) {
        self.page %= PAGES.len();
        let n = PAGES[self.page].knobs.len();
        if self.row >= n {
            self.row = n - 1;
        }
        self.col %= Element::COUNT;
    }
}

/// The per-frame numbers the header reports, gathered by the caller because
/// they are about the *runner*, not the world.
pub struct Run {
    pub paused: bool,
    pub speed: u64,
    pub realtime: Option<u64>,
    pub retuned: bool,
    pub wander: u32,
}

pub fn draw(out: &mut impl Write, v: &mut View, w: &World, t: &Tuning, run: &Run, rows: usize, cols: usize) {
    let cols = cols.max(60);
    let _ = write!(out, "\x1b[H");
    header(out, w, run, cols);

    match v.mode {
        Mode::Tune => {
            let knob_rows = v.knobs().len();
            // Everything except the map has a fixed height; the map gets what
            // is left, and turns itself off below usefulness.
            let fixed = 1 + 1 + 5 + 1 + 1 + knob_rows + 1 + 2;
            let map_h = if v.show_map { rows.saturating_sub(fixed + 3) } else { 0 };
            let map_h = if map_h < 4 { 0 } else { map_h.min(24) };
            if map_h > 0 {
                map(out, w, map_h, cols, None, None);
            }
            races(out, v, w, t, cols);
            grid(out, v, t, cols);
            footer(out, v, t, run, cols);
        }
        Mode::Soul => {
            let evs = View::sorted_events(w);
            v.sel = v.sel.min(evs.len().saturating_sub(1));
            let panel_h = 9;
            let map_h = rows.saturating_sub(2 + panel_h).clamp(6, 44);
            let sel_cell = evs.get(v.sel).map(|e| (e.cell as usize, e.element));
            map(out, w, map_h, cols, sel_cell, None);
            panel(out, v, w, &evs);
            let _ = writeln!(
                out,
                "{DIM}↑↓ choose · enter incarnate · 1-5 next of race · s knobs · \
                 space pause · </> speed · q quit{RESET}{CLEAR_EOL}"
            );
            notice_line(out, v);
        }
        Mode::Body(id) => {
            let map_h = rows.saturating_sub(2 + 4).clamp(6, 44);
            map(out, w, map_h, cols, None, Some(id));
            hud(out, w, id);
            let _ = writeln!(
                out,
                "{DIM}←↑↓→ steer · esc release the body · space pause · </> speed · \
                 q quit{RESET}{CLEAR_EOL}"
            );
            notice_line(out, v);
        }
    }

    let _ = write!(out, "\x1b[J");
    let _ = out.flush();
}

fn notice_line(out: &mut impl Write, v: &View) {
    if let Some((m, _)) = &v.notice {
        let _ = writeln!(out, "{BRIGHT}{m}{RESET}{CLEAR_EOL}");
    } else {
        let _ = writeln!(out, "{CLEAR_EOL}");
    }
}

fn header(out: &mut impl Write, w: &World, run: &Run, _cols: usize) {
    let min = w.tick / pentagram::race::TERRAIN_PERIOD;
    let rate = match run.realtime {
        Some(x) => format!("{x}× real"),
        None => "measuring…".into(),
    };
    let state = if run.paused {
        format!("{BRIGHT}▮▮ PAUSED{RESET}")
    } else {
        format!("{DIM}▶{RESET} {} t/s", run.speed)
    };
    let tuned = if run.retuned {
        format!("  {BRIGHT}✎ retuned{RESET}")
    } else {
        String::new()
    };
    let _ = writeln!(
        out,
        "{BRIGHT}CHAOTIC NATURE{RESET}{DIM} · live{RESET}   tick {}  ·  {}d {:02}h {:02}m sim  ·  \
         {}  ·  {}  ·  {} alive{}{CLEAR_EOL}",
        grouped(w.tick as i64),
        min / 1440,
        (min / 60) % 24,
        min % 60,
        state,
        rate,
        w.alive_count(),
        tuned
    );
}

/// What one sampled half-cell shows: a body (bright), terrain (dim tint), or
/// nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Half {
    Empty,
    Terrain((u8, u8, u8)),
    Body((u8, u8, u8)),
}

impl Half {
    fn color(self) -> Option<(u8, u8, u8)> {
        match self {
            Half::Empty => None,
            Half::Terrain(c) | Half::Body(c) => Some(c),
        }
    }
}

/// Saturation as a dim wash of the element's colour: barely-there at trace
/// levels, unmistakable near capacity, never bright enough to fight a body.
fn tint(e: Element, sat: u16) -> Option<(u8, u8, u8)> {
    if sat < 300 {
        return None;
    }
    let c = RGB[e];
    let b = 26 + (sat as u32 * 105) / 65_535; // 26..131
    Some((
        (c.0 as u32 * b / 190) as u8,
        (c.1 as u32 * b / 190) as u8,
        (c.2 as u32 * b / 190) as u8,
    ))
}

/// The world, at two sampled rows per terminal row: terrain as a coloured
/// wash underneath, bodies bright on top, incarnation events as markers.
///
/// A terminal cell is about twice as tall as it is wide, so half-block glyphs
/// buy back exactly the vertical resolution that costs — the sampled grid
/// comes out square. `▀` paints the upper half in the foreground colour and
/// the lower half in the background colour, so one glyph carries two cells.
fn map(
    out: &mut impl Write,
    w: &World,
    h: usize,
    cols: usize,
    selected: Option<(usize, Element)>,
    player: Option<u32>,
) {
    let mw = (h * 2).min(cols.saturating_sub(4));
    let mh = h * 2;
    let side = w.size.floor_int().max(1) as usize;

    // Bodies, bucketed into half-cells.
    let mut bodies = vec![[0u16; Element::COUNT]; mw * mh];
    for e in &w.entities {
        if !e.alive {
            continue;
        }
        let x = ((e.pos.x.floor_int().max(0) as usize) * mw / side).min(mw - 1);
        let y = ((e.pos.y.floor_int().max(0) as usize) * mh / side).min(mh - 1);
        bodies[y * mw + x][e.element.index()] += 1;
    }

    let half = |hr: usize, c: usize| -> Half {
        let cell = &bodies[hr * mw + c];
        let total: u16 = cell.iter().sum();
        if total > 0 {
            let best = cell
                .iter()
                .enumerate()
                .max_by_key(|(_, n)| **n)
                .map(|(i, _)| i)
                .unwrap_or(0);
            return Half::Body(RGB.0[best]);
        }
        // Point-sample the terrain under this half-cell.
        let tx = (c * side / mw).min(side - 1);
        let ty = (hr * side / mh).min(side - 1);
        let (el, sat) = w.terrain.dominant(ty * side + tx);
        match tint(el, sat) {
            Some(c) => Half::Terrain(c),
            None => Half::Empty,
        }
    };

    // Overlays: events (and the player) drawn over whatever is under them.
    // (char row, char col) → (glyph, colour, inverted)
    let to_char = |cell: usize| -> (usize, usize) {
        let ex = cell % side;
        let ey = cell / side;
        ((ey * mh / side) / 2, (ex * mw / side).min(mw - 1))
    };
    /// An overlay glyph: (char row, char col, glyph, colour, inverted).
    type Mark = (usize, usize, char, (u8, u8, u8), bool);
    let mut marks: Vec<Mark> = Vec::new();
    for ev in &w.events {
        let (r, c) = to_char(ev.cell as usize);
        let sel = selected == Some((ev.cell as usize, ev.element));
        marks.push((r, c, '✶', RGB[ev.element], sel));
    }
    if let Some(id) = player {
        if let Ok(i) = w.entities.binary_search_by_key(&id, |e| e.id) {
            let p = w.entities[i].pos;
            let cell = w.terrain.cell_of(p);
            let (r, c) = to_char(cell);
            marks.push((r, c, '@', (255, 255, 255), false));
        }
    }

    let _ = writeln!(out, "{DIM}┌{}┐{RESET}{CLEAR_EOL}", "─".repeat(mw));
    for r in 0..h {
        let _ = write!(out, "{DIM}│{RESET}");
        for c in 0..mw {
            if let Some((_, _, g, mc, sel)) = marks.iter().find(|(mr, mc2, ..)| *mr == r && *mc2 == c) {
                let bg = half(r * 2 + 1, c).color();
                let inv = if *sel { INVERT } else { "" };
                match bg {
                    Some(b) => {
                        let _ = write!(
                            out,
                            "{inv}\x1b[48;2;{};{};{}m{}{g}\x1b[0m",
                            b.0, b.1, b.2, col(*mc)
                        );
                    }
                    None => {
                        let _ = write!(out, "{inv}\x1b[49m{}{g}\x1b[0m", col(*mc));
                    }
                }
                continue;
            }
            let top = half(r * 2, c);
            let bot = half(r * 2 + 1, c);
            match (top.color(), bot.color()) {
                (None, None) => {
                    let _ = write!(out, "\x1b[0m ");
                }
                (Some(tc), None) => {
                    let _ = write!(out, "\x1b[49m{}▀", col(tc));
                }
                (None, Some(bc)) => {
                    let _ = write!(out, "\x1b[49m{}▄", col(bc));
                }
                (Some(tc), Some(bc)) => {
                    let _ = write!(out, "\x1b[48;2;{};{};{}m{}▀", bc.0, bc.1, bc.2, col(tc));
                }
            }
        }
        let _ = writeln!(out, "\x1b[0m{DIM}│{RESET}{CLEAR_EOL}");
    }
    let _ = writeln!(out, "{DIM}└{}┘{RESET}{CLEAR_EOL}", "─".repeat(mw));
}

/// The soul's choice: open incarnation events, soonest-closing first.
fn panel(out: &mut impl Write, v: &View, w: &World, evs: &[Event]) {
    let _ = writeln!(
        out,
        "{DIM}── incarnation events ─────────────────────────────{RESET}{CLEAR_EOL}"
    );
    if evs.is_empty() {
        let _ = writeln!(
            out,
            "{DIM}   none open — the world is between moments; wild sparks come on their own{RESET}{CLEAR_EOL}"
        );
    }
    let side = w.size.floor_int().max(1) as u32;
    for (i, ev) in evs.iter().take(6).enumerate() {
        let c = col(RGB[ev.element]);
        let left = ev.closes.saturating_sub(w.tick);
        let x = ev.cell % side;
        let y = ev.cell / side;
        let cursor = if i == v.sel { "▸" } else { " " };
        let line = format!(
            "{cursor} {c}✶ {:<6}{RESET} at ({:>3},{:>3})   closes in {:<8}",
            ev.element.name(),
            x,
            y,
            duration(left)
        );
        if i == v.sel {
            let _ = writeln!(out, "{BRIGHT}{line}{RESET}{CLEAR_EOL}");
        } else {
            let _ = writeln!(out, "{line}{CLEAR_EOL}");
        }
    }
    for _ in evs.len().min(6)..6 {
        let _ = writeln!(out, "{CLEAR_EOL}");
    }
}

/// Vitals while incarnated.
fn hud(out: &mut impl Write, w: &World, id: u32) {
    match w.entities.binary_search_by_key(&id, |e| e.id) {
        Ok(i) => {
            let e = &w.entities[i];
            let c = col(RGB[e.element]);
            let pct = (e.age * 100 / e.lifespan.max(1)).min(100);
            let filled = (pct as usize * 24 / 100).min(24);
            let bar: String = "▓".repeat(filled) + &"░".repeat(24 - filled);
            let _ = writeln!(
                out,
                "{c}@ {}{RESET} — hp {} · life {c}{bar}{RESET} {}% · {} of {} · at ({}, {}){CLEAR_EOL}",
                e.element.name(),
                e.hp,
                pct,
                duration(e.age),
                duration(e.lifespan),
                e.pos.x.floor_int(),
                e.pos.y.floor_int()
            );
        }
        Err(_) => {
            let _ = writeln!(out, "{DIM}between bodies…{RESET}{CLEAR_EOL}");
        }
    }
}

fn races(out: &mut impl Write, v: &View, w: &World, t: &Tuning, _cols: usize) {
    let _ = writeln!(
        out,
        "{DIM}race    alive  population        deposit: floor╵ nominal┊ granted▓ ceiling→ \
         │      granted state{RESET}{CLEAR_EOL}"
    );

    let peak = Element::ALL
        .iter()
        .flat_map(|e| v.history[*e].iter().copied())
        .max()
        .unwrap_or(1)
        .max(1);

    let pop = w.population();
    for e in Element::ALL {
        let g = w.last_deposit[e];
        let band = t.races[e].deposit;
        let c = col(RGB[e]);
        let state = if pop[e] == 0 {
            format!("{DIM}extinct — still churning at its floor{RESET}")
        } else if g.clipped > 0 {
            format!("{c}rate-limited{RESET}{DIM} — {} refused{RESET}", abbrev(g.clipped as i64))
        } else if g.forced > 0 {
            format!("{DIM}idle — floor is doing the work{RESET}")
        } else {
            format!("{DIM}inside its band{RESET}")
        };
        let _ = writeln!(
            out,
            "{c}{:<7}{RESET}{:>5}  {c}{:<16}{RESET} {} {:>7}/{:<7} {}{CLEAR_EOL}",
            e.name(),
            pop[e],
            spark(&v.history[e], peak, 16),
            gauge(g.granted, band, &c, 26),
            g.granted,
            band.ceiling,
            state
        );
    }
}

/// The band, drawn to scale from 0 to the ceiling: filled to `granted`, with
/// the floor and nominal marked in place. This is the whole governor story in
/// one line — where the rate is allowed to sit, and where it actually is.
fn gauge(granted: u64, b: RateBand, c: &str, w: usize) -> String {
    let ceil = b.ceiling.max(1) as u64;
    let at = |x: u64| ((x * w as u64) / ceil).min(w as u64 - 1) as usize;
    let fill = ((granted * w as u64) / ceil).min(w as u64) as usize;

    let mut chars = vec!['░'; w];
    for ch in chars.iter_mut().take(fill) {
        *ch = '▓';
    }
    chars[at(b.floor as u64)] = '╵';
    chars[at(b.nominal as u64)] = '┊';

    let lit: String = chars[..fill].iter().collect();
    let rest: String = chars[fill..].iter().collect();
    format!("{DIM}│{c}{lit}{DIM}{rest}│{RESET}")
}

fn spark(h: &[u32], max: u32, w: usize) -> String {
    let start = h.len().saturating_sub(w);
    let mut s: String = h[start..]
        .iter()
        .map(|v| SPARK[(((*v as u64 * 7) / max.max(1) as u64).min(7)) as usize])
        .collect();
    while s.chars().count() < w {
        s.insert(0, ' ');
    }
    s
}

fn grid(out: &mut impl Write, v: &View, t: &Tuning, cols: usize) {
    let knobs = v.knobs();
    let cw = ((cols.saturating_sub(20)) / 5).clamp(8, 14);

    let _ = writeln!(
        out,
        "{DIM}── knobs ── {}   (tab: page {}/{}){RESET}{CLEAR_EOL}",
        PAGES[v.page].title,
        v.page + 1,
        PAGES.len()
    );

    let mut hdr = format!("{:<18}", "");
    for e in Element::ALL {
        hdr.push_str(&format!("{}{:>w$}{RESET}", col(RGB[e]), e.name(), w = cw));
    }
    let _ = writeln!(out, "{hdr}{CLEAR_EOL}");

    for (i, k) in knobs.iter().enumerate() {
        let selected_row = i == v.row;
        let label = if selected_row {
            format!("{BRIGHT}{:<18}{RESET}", k.name)
        } else {
            format!("{DIM}{:<18}{RESET}", k.name)
        };
        let mut line = label;
        for e in Element::ALL {
            let cell = k.short(k.value(t, e));
            if selected_row && e.index() == v.col {
                line.push_str(&format!("{INVERT}{:>w$}{RESET}", cell, w = cw));
            } else if selected_row {
                line.push_str(&format!("{BRIGHT}{:>w$}{RESET}", cell, w = cw));
            } else {
                line.push_str(&format!("{:>w$}", cell, w = cw));
            }
        }
        let _ = writeln!(out, "{line}{CLEAR_EOL}");
    }

    // §3.1, computed rather than hoped for. `deposit_unit / lifespan` is what
    // keeps a race that lives eight minutes from reshaping the map faster than
    // one that lives a fortnight, and it is the first thing a lifespan or
    // deposit-unit edit breaks.
    let p: Vec<u64> = Element::ALL
        .iter()
        .map(|e| t.races[*e].terraform_pressure())
        .collect();
    let (lo, hi) = (
        p.iter().copied().min().unwrap_or(1).max(1),
        p.iter().copied().max().unwrap_or(1),
    );
    let spread = hi * 100 / lo;
    let verdict = if spread <= 200 {
        format!("{DIM}spread {}.{:02}× — inside parity{RESET}", spread / 100, spread % 100)
    } else {
        format!("{BRIGHT}spread {}.{:02}× — OUTSIDE the 2× parity band{RESET}", spread / 100, spread % 100)
    };
    let mut line = format!("{DIM}{:<18}{RESET}", "pressure");
    for e in Element::ALL {
        line.push_str(&format!("{DIM}{:>w$}{RESET}", p[e.index()], w = cw));
    }
    let _ = writeln!(out, "{line}  {verdict}{CLEAR_EOL}");
}

fn footer(out: &mut impl Write, v: &View, t: &Tuning, run: &Run, _cols: usize) {
    let k = &v.knobs()[v.row];
    let e = v.element();
    let stepping = match k.step {
        crate::knobs::Step::Add(n) => format!("-/+ ±{n}   [/] ±{}", n * 10),
        crate::knobs::Step::Scale => "-/+ ±10%   [/] ×2".to_string(),
    };
    let _ = writeln!(
        out,
        "{}▸ {} {}{RESET} = {BRIGHT}{}{RESET}   {DIM}{}   ·   {}{RESET}{CLEAR_EOL}",
        col(RGB[e]),
        e.name(),
        k.name,
        k.long(k.value(t, e)),
        stepping,
        k.help
    );

    match &v.notice {
        Some((m, _)) => {
            let _ = writeln!(out, "{BRIGHT}{m}{RESET}{CLEAR_EOL}");
        }
        None => {
            let _ = writeln!(
                out,
                "{DIM}↑↓←→/hjkl move · -/+ [/] adjust · space pause · < > speed {} t/s · \
                 . advance 1 min · w wander {}% · tab page · m map · r/R reset · z restart · \
                 T write table · q quit{RESET}{CLEAR_EOL}",
                run.speed, run.wander
            );
        }
    }
}
