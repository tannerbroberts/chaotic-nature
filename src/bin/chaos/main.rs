//! Chaotic Nature — the live view.
//!
//! The tuning loop this project is built around, with the table on the screen
//! instead of in an editor: move the cursor to a number, change it, watch what
//! the world does about it.
//!
//! ## What is and is not deterministic here
//!
//! Retuning mid-run is a *deliberate* break with the replay story. `verify` and
//! `soak` run the shipped table start to finish and reproduce bit-identically;
//! this view lets you change the table under a running world, which no recorded
//! trace could reproduce. That is the trade the instrument exists to make, and
//! the header says `✎ retuned` for as long as the world has been touched. Press
//! `z` to restart from tick 0 with the current knobs and get a clean run back.
//!
//! Restocking and wander are not simulation rules either — they are input
//! commands the view submits, exactly as a player would. Stage 0 has no
//! reproduction and no goals, so without them the map empties out and the
//! survivors travel in straight lines forever.
//!
//! Zero dependencies, ANSI escapes only.

// Presentation only. The simulation this drives never sees a float.
#![allow(clippy::float_arithmetic)]

mod knobs;
mod term;
mod view;

use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

use pentagram::element::Element;
use pentagram::fx::{Fx, V2};
use pentagram::input::{CmdKind, Command, InputLog};
use pentagram::race::{RACES, TERRAIN_PERIOD};
use pentagram::rand::{rand_below, rand_signed, Channel};
use pentagram::world::World;

use knobs::Tuning;
use term::{Key, Keys, Term};
use view::{Mode, Run, View};

/// 20 frames a second. Sim speed is decoupled from this — a frame runs however
/// many ticks the speed knob asks for.
const FRAME: Duration = Duration::from_millis(50);
/// However fast the speed knob is set, one frame gets this long to simulate
/// before we draw anyway. Without it, winding the speed past what the machine
/// can do would lock the keyboard out.
const FRAME_BUDGET: Duration = Duration::from_millis(40);
/// Ceiling on bodies of one race the restock knob may spawn in a single frame.
/// Restocking 200 at once would be a stampede, not a population — but the
/// allowance has to scale with how much time a frame covers, or a race with an
/// eight-minute lifespan quietly dies out whenever the sim speed is wound up.
const RESTOCK_MAX: u64 = 24;

const WANDER_STEPS: [u32; 7] = [0, 1, 2, 5, 10, 25, 50];

struct Args {
    size: i32,
    pop: u32,
    speed: u64,
    seed: u64,
    rows: Option<usize>,
    cols: Option<usize>,
}

impl Args {
    fn parse() -> Args {
        let mut a = Args {
            size: 96,
            pop: 60,
            speed: 120,
            seed: 0xBEEF,
            rows: None,
            cols: None,
        };
        let argv: Vec<String> = std::env::args().skip(1).collect();
        for (i, arg) in argv.iter().enumerate() {
            let v = argv.get(i + 1).and_then(|s| s.parse::<i64>().ok());
            match arg.as_str() {
                "--size" => a.size = v.unwrap_or(96).clamp(16, 4096) as i32,
                "--pop" => a.pop = v.unwrap_or(60).clamp(0, 250) as u32,
                "--speed" => a.speed = v.unwrap_or(120).clamp(1, 200_000) as u64,
                "--seed" => a.seed = v.unwrap_or(0xBEEF) as u64,
                "--rows" => a.rows = v.map(|n| n.clamp(20, 200) as usize),
                "--cols" => a.cols = v.map(|n| n.clamp(60, 400) as usize),
                "--help" | "-h" => {
                    println!(
                        "chaos — Chaotic Nature\n\n\
                         You arrive as a soul over a living map. Incarnation events open as\n\
                         terrain allows — and as wild sparks; pick one, live it, die, return.\n\
                         Every race and terrain attribute is editable while it runs; flags only\n\
                         set the starting conditions.\n\n\
                         options:\n  \
                         --size N    world edge in cells        (default 96)\n  \
                         --pop N     bodies per race to seed    (default 60)\n  \
                         --speed N   sim ticks per second       (default 120, real time is 1.67)\n  \
                         --seed N    world seed                 (default 0xBEEF)\n  \
                         --rows N    force view height          (default: ask the terminal)\n  \
                         --cols N    force view width\n\n\
                         soul view:\n  \
                         ↑↓          choose an incarnation event    enter   incarnate\n  \
                         1-5         jump to next event of a race   s       knob table\n\n\
                         incarnated:\n  \
                         ↑↓←→        steer your body                esc     release the body\n\n\
                         knob table:\n  \
                         ↑↓←→ or hjkl   move the cursor (rows are knobs, columns are races)\n  \
                         - + / [ ]      adjust (coarse)             tab     page (rates·mix·terrain)\n  \
                         r / R          reset knob / whole table    m       show or hide the map\n  \
                         w              cycle wander steering       T       write src/race.tuned.rs\n  \
                         z              restart at tick 0 with the current knobs\n\n\
                         everywhere:\n  \
                         space pause · . advance one sim minute · < > speed · q quit\n\n\
                         subcommands (via the wrapper): verify · soak · test · edit · watch"
                    );
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        a
    }
}

/// Everything the runner owns that is not the world or the knobs.
struct Sim {
    speed: u64,
    paused: bool,
    /// Fractional tick carry, so speeds below the frame rate still advance.
    carry: u64,
    /// Ticks queued by `.` while paused.
    stepping: u64,
    wander: usize,
    /// Ticks simulated and wall time spent, for the "× real time" readout.
    ticked: u64,
    since: Instant,
    retuned: bool,
    /// Player commands waiting to be stamped into the next input log —
    /// `(entity, kind)`, stamped at drain time so pausing cannot strand them
    /// in the past.
    pending: Vec<(u32, CmdKind)>,
    /// An `Incarnate` has been submitted for this event id; watching
    /// `World::last_claim` for the body it produces.
    await_claim: Option<u32>,
    /// Tick the current body was born, for the "lived" figure at death.
    body_since: u64,
}

fn main() {
    let a = Args::parse();
    let mut t = Tuning::new(a.pop);
    let mut w = World::new(a.seed, a.size);
    w.seed_population(a.pop);

    let mut v = View::new();
    let mut sim = Sim {
        speed: a.speed,
        paused: false,
        carry: 0,
        stepping: 0,
        wander: 3,
        ticked: 0,
        since: Instant::now(),
        retuned: false,
        pending: Vec::new(),
        await_claim: None,
        body_since: 0,
    };

    let term = Term::enter();
    let mut keys = Keys::spawn();
    let mut out = BufWriter::new(std::io::stdout());

    'outer: loop {
        let frame_start = Instant::now();

        for k in keys.poll() {
            if handle(k, &mut v, &mut t, &mut sim, &mut w, a.seed, a.size) {
                break 'outer;
            }
        }
        if keys.closed {
            break;
        }

        // The knobs are the authority; push them at the world every frame. It
        // is a 5-row copy, so there is no point being clever about when.
        w.retune(t.races);
        w.retune_terrain(t.terrain);

        let ticks = if sim.stepping > 0 {
            let n = sim.stepping.min(TERRAIN_PERIOD);
            sim.stepping -= n;
            n
        } else if sim.paused {
            0
        } else {
            sim.carry += sim.speed;
            let n = sim.carry / 20;
            sim.carry %= 20;
            n
        };

        if ticks > 0 {
            let player = match v.mode {
                Mode::Body(id) => Some(id),
                _ => None,
            };
            let log = inputs(&w, &t, WANDER_STEPS[sim.wander], a.seed, ticks, &mut sim.pending, player);
            for _ in 0..ticks {
                w.step(&log);
                sim.ticked += 1;
                if frame_start.elapsed() > FRAME_BUDGET {
                    break;
                }
            }
            resolve(&mut v, &mut sim, &w);
        }

        v.sample(&w);
        let secs = sim.since.elapsed().as_secs_f64();
        let run = Run {
            paused: sim.paused,
            speed: sim.speed,
            realtime: if secs > 1.0 {
                Some((sim.ticked as f64 / secs / 1.667) as u64)
            } else {
                None
            },
            retuned: sim.retuned,
            wander: WANDER_STEPS[sim.wander],
        };

        let (rows, cols) = term.size().unwrap_or((40, 100));
        view::draw(
            &mut out,
            &mut v,
            &w,
            &t,
            &run,
            a.rows.unwrap_or(rows),
            a.cols.unwrap_or(cols),
        );

        if let Some((_, n)) = &mut v.notice {
            *n -= 1;
            if *n == 0 {
                v.notice = None;
            }
        }

        if let Some(rest) = FRAME.checked_sub(frame_start.elapsed()) {
            std::thread::sleep(rest);
        }
    }

    // Order matters: flush our last frame, then hand the terminal back.
    let _ = out.flush();
    drop(out);
    drop(term);
}

/// Returns true when the view should quit.
fn handle(
    k: Key,
    v: &mut View,
    t: &mut Tuning,
    sim: &mut Sim,
    w: &mut World,
    seed: u64,
    size: i32,
) -> bool {
    // Keys that mean the same thing on every screen.
    match k {
        Key::Char('q') | Key::Char('\x03') => return true,
        Key::Char('<') | Key::Char(',') => {
            sim.speed = (sim.speed / 2).max(1);
            return false;
        }
        Key::Char('>') => {
            sim.speed = (sim.speed * 2).min(200_000);
            return false;
        }
        Key::Char(' ') => {
            sim.paused = !sim.paused;
            return false;
        }
        // Freeze, then advance exactly one terrain tick — the granularity the
        // governors actually settle at, so every readout moves once.
        Key::Char('.') => {
            sim.paused = true;
            sim.stepping = TERRAIN_PERIOD;
            return false;
        }
        _ => {}
    }

    match v.mode {
        Mode::Tune => handle_tune(k, v, t, sim, w, seed, size),
        Mode::Soul => handle_soul(k, v, sim, w),
        Mode::Body(id) => handle_body(k, v, sim, id),
    }
    false
}

fn handle_tune(k: Key, v: &mut View, t: &mut Tuning, sim: &mut Sim, w: &mut World, seed: u64, size: i32) {
    let knobs = v.knobs();
    let e = v.element();

    match k {
        Key::Char('s') | Key::Esc => {
            v.mode = Mode::Soul;
            return;
        }

        Key::Up | Key::Char('k') => v.row = v.row.checked_sub(1).unwrap_or(knobs.len() - 1),
        Key::Down | Key::Char('j') => v.row = (v.row + 1) % knobs.len(),
        Key::Left | Key::Char('h') => v.col = (v.col + Element::COUNT - 1) % Element::COUNT,
        Key::Right | Key::Char('l') => v.col = (v.col + 1) % Element::COUNT,

        Key::Char('-') | Key::Char('_') => {
            knobs[v.row].nudge(t, e, false, false);
            sim.retuned = true;
        }
        Key::Char('+') | Key::Char('=') => {
            knobs[v.row].nudge(t, e, true, false);
            sim.retuned = true;
        }
        Key::Char('[') | Key::ShiftLeft => {
            knobs[v.row].nudge(t, e, false, true);
            sim.retuned = true;
        }
        Key::Char(']') | Key::ShiftRight => {
            knobs[v.row].nudge(t, e, true, true);
            sim.retuned = true;
        }

        Key::Char('\t') => {
            v.page = (v.page + 1) % View::pages();
            v.row = 0;
        }
        Key::Char('m') => v.show_map = !v.show_map,
        Key::Char('w') => {
            sim.wander = (sim.wander + 1) % WANDER_STEPS.len();
            let pct = WANDER_STEPS[sim.wander];
            v.say(format!(
                "wander {pct}% — the view re-steers that share of bodies each second"
            ));
        }

        Key::Char('r') => {
            let shipped = Tuning::new(t.restock[e]);
            let val = knobs[v.row].value(&shipped, e);
            (knobs[v.row].set)(t, e, val);
            v.say(format!("{} {} back to the shipped value", e.name(), knobs[v.row].name));
        }
        Key::Char('R') => {
            *t = Tuning { races: RACES, terrain: pentagram::terrain::TERRAIN, restock: t.restock, speed_mult: 1000 };
            sim.retuned = false;
            v.say("whole table back to the shipped values");
        }
        Key::Char('z') => {
            *w = World::new(seed, size);
            w.retune(t.races);
            w.retune_terrain(t.terrain);
            for el in Element::ALL {
                for _ in 0..t.restock[el] {
                    let x = rand_below(seed, w.tick, w.next_id, Channel::SpawnPlacement, size.max(1) as u32);
                    let y = rand_below(seed, w.tick, w.next_id ^ 0x5A5A, Channel::SpawnPlacement, size.max(1) as u32);
                    w.spawn(el, V2::new(Fx::from_int(x as i32), Fx::from_int(y as i32)));
                }
            }
            sim.ticked = 0;
            sim.since = Instant::now();
            sim.retuned = false;
            sim.pending.clear();
            sim.await_claim = None;
            v.mode = Mode::Soul;
            v.history = pentagram::element::PerElement([vec![], vec![], vec![], vec![], vec![]]);
            v.say("restarted at tick 0 — this run is reproducible from these knobs");
        }
        Key::Char('T') => match knobs::write_table(t) {
            Ok(p) => v.say(format!("wrote {p}  (diff it against src/race.rs)")),
            Err(e) => v.say(format!("could not write the table: {e}")),
        },

        _ => {}
    }
    v.clamp();
}

fn handle_soul(k: Key, v: &mut View, sim: &mut Sim, w: &World) {
    let evs = View::sorted_events(w);
    match k {
        Key::Char('s') | Key::Esc | Key::Char('\t') => v.mode = Mode::Tune,

        Key::Up | Key::Char('k') => {
            let n = evs.len().max(1);
            v.sel = v.sel.checked_sub(1).unwrap_or(n - 1);
        }
        Key::Down | Key::Char('j') => {
            let n = evs.len().max(1);
            v.sel = (v.sel + 1) % n;
        }

        // Jump to the next open event of a specific race.
        Key::Char(c @ '1'..='5') => {
            let el = Element::from_index(c as usize - '1' as usize);
            let n = evs.len();
            if n > 0 {
                let hit = (1..=n)
                    .map(|off| (v.sel + off) % n)
                    .find(|i| evs[*i].element == el);
                match hit {
                    Some(i) => v.sel = i,
                    None => v.say(format!("no open {} events right now", el.name())),
                }
            }
        }

        Key::Char('\r') | Key::Char('\n') => {
            if sim.await_claim.is_some() {
                return;
            }
            match evs.get(v.sel) {
                Some(ev) => {
                    sim.pending.push((0, CmdKind::Incarnate { event: ev.id }));
                    sim.await_claim = Some(ev.id);
                    if sim.paused {
                        v.say(format!(
                            "reaching for the {} moment — unpause to be born",
                            ev.element.name()
                        ));
                    } else {
                        v.say(format!("reaching for the {} moment…", ev.element.name()));
                    }
                }
                None => v.say("no open events — the world is between moments"),
            }
        }

        _ => {}
    }
}

fn handle_body(k: Key, v: &mut View, sim: &mut Sim, id: u32) {
    let dir = |x: i32, y: i32| V2::new(Fx::from_int(x), Fx::from_int(y));
    match k {
        Key::Esc => {
            v.mode = Mode::Soul;
            v.say("the body wanders on without you");
        }
        Key::Up | Key::Char('k') => sim.pending.push((id, CmdKind::SetHeading { dir: dir(0, -1) })),
        Key::Down | Key::Char('j') => sim.pending.push((id, CmdKind::SetHeading { dir: dir(0, 1) })),
        Key::Left | Key::Char('h') => sim.pending.push((id, CmdKind::SetHeading { dir: dir(-1, 0) })),
        Key::Right | Key::Char('l') => sim.pending.push((id, CmdKind::SetHeading { dir: dir(1, 0) })),
        _ => {}
    }
}

/// The soul → body → soul transitions that depend on what the world just did.
fn resolve(v: &mut View, sim: &mut Sim, w: &World) {
    if let Some(eid) = sim.await_claim {
        match w.last_claim {
            Some((cev, cent)) if cev == eid => {
                sim.await_claim = None;
                sim.body_since = w.tick;
                v.mode = Mode::Body(cent);
                let el = w
                    .entities
                    .binary_search_by_key(&cent, |e| e.id)
                    .ok()
                    .map(|i| w.entities[i].element.name())
                    .unwrap_or("?");
                if sim.speed > 20 {
                    sim.speed = 10;
                    v.say(format!(
                        "born as {el} — arrows steer · esc releases the body · slowed to 10 t/s"
                    ));
                } else {
                    v.say(format!("born as {el} — arrows steer · esc releases the body"));
                }
            }
            _ if !w.events.iter().any(|e| e.id == eid) => {
                sim.await_claim = None;
                v.say("the moment passed before you reached it");
            }
            _ => {}
        }
    }

    if let Mode::Body(id) = v.mode {
        if w.entities.binary_search_by_key(&id, |e| e.id).is_err() {
            v.mode = Mode::Soul;
            let lived = w.tick.saturating_sub(sim.body_since);
            v.say(format!(
                "your body returned to the land — {} lived · choose another moment",
                knobs::duration(lived)
            ));
        }
    }
}

/// The view's own input stream: the player's queued commands, plus restocking
/// and wander-steering. All ordinary commands stamped for the ticks about to
/// run — the same path any player's input takes.
fn inputs(
    w: &World,
    t: &Tuning,
    wander_pct: u32,
    seed: u64,
    ticks: u64,
    pending: &mut Vec<(u32, CmdKind)>,
    player: Option<u32>,
) -> InputLog {
    let mut log = InputLog::new();
    let tick = w.tick;
    let size = w.size.floor_int().max(1) as u32;
    let pop = w.population();

    // The player goes first, stamped for the first tick about to run.
    for (entity, kind) in pending.drain(..) {
        log.push(Command { tick, entity, kind });
    }

    let allowance = (ticks / 20).clamp(1, RESTOCK_MAX) as u32;
    for e in Element::ALL {
        let deficit = t.restock[e].saturating_sub(pop[e]).min(allowance);
        for k in 0..deficit {
            let salt = (e.index() as u32) * 7919 + k;
            let x = rand_below(seed, tick, salt, Channel::SpawnPlacement, size);
            let y = rand_below(seed, tick, salt ^ 0x5A5A, Channel::SpawnPlacement, size);
            log.push(Command {
                tick,
                entity: 0,
                kind: CmdKind::Spawn {
                    element: e,
                    at: V2::new(Fx::from_int(x as i32), Fx::from_int(y as i32)),
                },
            });
        }
    }

    // Re-steer `wander_pct` of the population per second, spread across the
    // ticks this frame covers. Without it bodies bounce between walls on
    // rails. The player's body is never wander-steered — those hands are
    // somebody's.
    let n = w.entities.len() as u64;
    if wander_pct > 0 && n > 0 {
        let steers = (n * wander_pct as u64 * ticks / 100 / 100).max(1).min(n);
        for s in 0..steers {
            let at = rand_below(seed, tick, s as u32, Channel::Wander, n as u32) as usize;
            let id = w.entities[at].id;
            if player == Some(id) {
                continue;
            }
            log.push(Command {
                tick: tick + (s * ticks / steers.max(1)),
                entity: id,
                kind: CmdKind::SetHeading {
                    dir: V2::new(
                        rand_signed(seed, tick, s as u32 + 1, Channel::Wander),
                        rand_signed(seed, tick, s as u32 + 2, Channel::Wander),
                    ),
                },
            });
        }
    }

    log.finalize();
    log
}
