//! Chaotic Nature — the windowed client.
//!
//! Mouse-first and self-explaining: hover anything to learn what it is, click
//! a birth-moment to take it, click the ground to steer the body you were
//! given. The terminal client (`chaos tui`) drives the identical simulation
//! and tuning table; this one exists because a map you can point at is worth
//! a page of documentation.
//!
//! `eframe` is a presentation dependency, quarantined to this binary. The
//! simulation library underneath remains zero-dependency and never sees a
//! float — everything here reads sim state and submits ordinary commands.

#![allow(clippy::float_arithmetic)]

use std::collections::VecDeque;
use std::time::Instant;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use pentagram::element::{Element, PerElement};
use pentagram::fx::{Fx, V2};
use pentagram::input::{CmdKind, Command, InputLog};
use pentagram::race::TERRAIN_PERIOD;
use pentagram::rand::{rand_below, rand_signed, Channel};
use pentagram::tuning::{duration, write_table, Knob, Tuning, PAGES, RGB};
use pentagram::world::{Event, World};

const HISTORY: usize = 900;
/// Ceiling on bodies of one race the restock logic spawns per frame.
const RESTOCK_MAX: u64 = 24;

fn main() -> eframe::Result {
    let a = Args::parse();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 880.0])
            .with_min_inner_size([900.0, 620.0])
            .with_title("Chaotic Nature"),
        ..Default::default()
    };
    eframe::run_native(
        "Chaotic Nature",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, a)))),
    )
}

struct Args {
    size: i32,
    pop: u32,
    speed: f64,
    seed: u64,
}

impl Args {
    fn parse() -> Args {
        let mut a = Args { size: 256, pop: 60, speed: 120.0, seed: 0xBEEF };
        let argv: Vec<String> = std::env::args().skip(1).collect();
        for (i, arg) in argv.iter().enumerate() {
            let v = argv.get(i + 1).and_then(|s| s.parse::<i64>().ok());
            match arg.as_str() {
                "--size" => a.size = v.unwrap_or(256).clamp(16, 4096) as i32,
                "--pop" => a.pop = v.unwrap_or(60).clamp(0, 250) as u32,
                "--speed" => a.speed = v.unwrap_or(120).clamp(1, 200_000) as f64,
                "--seed" => a.seed = v.unwrap_or(0xBEEF) as u64,
                _ => {}
            }
        }
        a
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Soul,
    Body(u32),
}

/// What the map is showing: every resource, or the world as one race sees it —
/// predators red, prey green, the indifferent in grayscale.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MapView {
    All,
    Race(Element),
}

struct App {
    w: World,
    t: Tuning,
    seed: u64,
    size: i32,

    mode: Mode,
    await_claim: Option<u32>,
    body_since: u64,
    /// Set at death, cleared by the modal's button.
    obituary: Option<String>,

    speed: f64,
    paused: bool,
    carry: f64,
    last: Instant,
    retuned: bool,
    wander_pct: u32,
    pending: Vec<(u32, CmdKind)>,
    last_steer: Instant,

    history: PerElement<VecDeque<u32>>,
    frame: u64,
    notice: Option<(String, Instant)>,

    terrain_tex: Option<egui::TextureHandle>,
    knob_element: Element,
    view: MapView,
    show_help: bool,
}

impl App {
    fn new(_cc: &eframe::CreationContext<'_>, a: Args) -> App {
        let t = Tuning::new(a.pop);
        let mut w = World::new(a.seed, a.size);
        w.seed_population(a.pop);
        App {
            w,
            t,
            seed: a.seed,
            size: a.size,
            mode: Mode::Soul,
            await_claim: None,
            body_since: 0,
            obituary: None,
            speed: a.speed,
            paused: false,
            carry: 0.0,
            last: Instant::now(),
            retuned: false,
            wander_pct: 5,
            pending: Vec::new(),
            last_steer: Instant::now(),
            history: PerElement([VecDeque::new(), VecDeque::new(), VecDeque::new(), VecDeque::new(), VecDeque::new()]),
            frame: 0,
            notice: None,
            terrain_tex: None,
            knob_element: Element::Fire,
            view: MapView::All,
            show_help: true,
        }
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), Instant::now()));
    }

    fn restart(&mut self, seed: u64) {
        self.seed = seed;
        self.w = World::new(seed, self.size);
        self.w.retune(self.t.races);
        self.w.retune_terrain(self.t.terrain);
        for el in Element::ALL {
            for _ in 0..self.t.restock[el] {
                let x = rand_below(seed, 0, self.w.next_id, Channel::SpawnPlacement, self.size.max(1) as u32);
                let y = rand_below(seed, 0, self.w.next_id ^ 0x5A5A, Channel::SpawnPlacement, self.size.max(1) as u32);
                self.w.spawn(el, V2::new(Fx::from_int(x as i32), Fx::from_int(y as i32)));
            }
        }
        self.mode = Mode::Soul;
        self.await_claim = None;
        self.pending.clear();
        self.obituary = None;
        for e in Element::ALL {
            self.history.get_mut(e).clear();
        }
    }

    // ------------------------------------------------------------------
    // Simulation driving — identical in spirit to the terminal client:
    // everything the view does to the world goes through input commands.
    // ------------------------------------------------------------------

    fn advance(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().min(0.25);
        self.last = now;
        if self.paused {
            // Queued player commands still land on the next unpause.
            return;
        }
        self.carry += dt * self.speed;
        let mut ticks = self.carry.floor() as u64;
        self.carry -= ticks as f64;
        ticks = ticks.min(50_000);
        if ticks == 0 {
            return;
        }

        self.w.retune(self.t.races);
        self.w.retune_terrain(self.t.terrain);
        self.w.set_speed_scale(self.t.speed_mult.min(u16::MAX as u32) as u16);

        let log = self.inputs(ticks);
        let budget = Instant::now();
        for _ in 0..ticks {
            self.w.step(&log);
            if budget.elapsed().as_millis() > 12 {
                break;
            }
        }
        self.resolve();
    }

    fn inputs(&mut self, ticks: u64) -> InputLog {
        let mut log = InputLog::new();
        let w = &self.w;
        let tick = w.tick;
        let size = w.size.floor_int().max(1) as u32;
        let pop = w.population();

        for (entity, kind) in self.pending.drain(..) {
            log.push(Command { tick, entity, kind });
        }

        let allowance = (ticks / 20).clamp(1, RESTOCK_MAX) as u32;
        for e in Element::ALL {
            let deficit = self.t.restock[e].saturating_sub(pop[e]).min(allowance);
            for k in 0..deficit {
                let salt = (e.index() as u32) * 7919 + k;
                let x = rand_below(self.seed, tick, salt, Channel::SpawnPlacement, size);
                let y = rand_below(self.seed, tick, salt ^ 0x5A5A, Channel::SpawnPlacement, size);
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

        let player = match self.mode {
            Mode::Body(id) => Some(id),
            Mode::Soul => None,
        };
        let n = w.entities.len() as u64;
        if self.wander_pct > 0 && n > 0 {
            let steers = (n * self.wander_pct as u64 * ticks / 100 / 100).max(1).min(n);
            for s in 0..steers {
                let at = rand_below(self.seed, tick, s as u32, Channel::Wander, n as u32) as usize;
                let id = w.entities[at].id;
                if player == Some(id) {
                    continue;
                }
                log.push(Command {
                    tick: tick + (s * ticks / steers.max(1)),
                    entity: id,
                    kind: CmdKind::SetHeading {
                        dir: V2::new(
                            rand_signed(self.seed, tick, s as u32 + 1, Channel::Wander),
                            rand_signed(self.seed, tick, s as u32 + 2, Channel::Wander),
                        ),
                    },
                });
            }
        }

        log.finalize();
        log
    }

    fn resolve(&mut self) {
        if let Some(eid) = self.await_claim {
            match self.w.last_claim {
                Some((cev, cent)) if cev == eid => {
                    self.await_claim = None;
                    self.body_since = self.w.tick;
                    self.mode = Mode::Body(cent);
                    if self.speed > 20.0 {
                        self.speed = 10.0;
                    }
                    let el = self
                        .w
                        .entities
                        .binary_search_by_key(&cent, |e| e.id)
                        .ok()
                        .map(|i| self.w.entities[i].element);
                    if let Some(el) = el {
                        self.view = MapView::Race(el);
                        self.say(format!(
                            "Born as {}. Green feeds you, red hunts you. Click the ground to move.",
                            el.name()
                        ));
                    }
                }
                _ if !self.w.events.iter().any(|e| e.id == eid) => {
                    self.await_claim = None;
                    self.say("The moment passed before you reached it.");
                }
                _ => {}
            }
        }

        if let Mode::Body(id) = self.mode {
            if self.w.entities.binary_search_by_key(&id, |e| e.id).is_err() {
                self.mode = Mode::Soul;
                let lived = duration(self.w.tick.saturating_sub(self.body_since));
                self.obituary = Some(format!(
                    "Your body returned to the land after {lived} of life.\n\
                     Its remains feed the ground it fell on — watch that spot."
                ));
            }
        }
    }
}

// ----------------------------------------------------------------------
// Colours.
// ----------------------------------------------------------------------

fn ecolor(e: Element) -> Color32 {
    let c = RGB[e];
    Color32::from_rgb(c.0, c.1, c.2)
}

const HUNTS_YOU: Color32 = Color32::from_rgb(228, 78, 64);
const FEEDS_YOU: Color32 = Color32::from_rgb(96, 198, 100);
const INDIFFERENT: Color32 = Color32::from_rgb(148, 150, 156);

/// An element's colour under the current lens. In a race view the world is
/// sorted by what it means to that race: the one that eats you is red, the
/// one you eat is green, you are yourself, and the rest fade to grayscale.
fn view_color(view: MapView, e: Element) -> Color32 {
    match view {
        MapView::All => ecolor(e),
        MapView::Race(r) => {
            if e == r {
                ecolor(e)
            } else if e == r.eats() {
                FEEDS_YOU
            } else if e == r.eaten_by() {
                HUNTS_YOU
            } else {
                INDIFFERENT
            }
        }
    }
}

const GROUND: Color32 = Color32::from_rgb(16, 18, 24);

/// Saturation buckets, shared by rendering and (eventually) tile-asset
/// mapping: a cell is its strongest element at one of four strengths. This is
/// the exact scheme a Godot TileMap would use — 5 elements × 4 buckets is 20
/// tiles, with boundaries drawn by autotiling, not by unique art per
/// combination.
const BUCKETS: [u16; 4] = [300, 4_000, 14_000, 32_000];

fn bucket(sat: u16) -> Option<usize> {
    if sat < BUCKETS[0] {
        return None;
    }
    Some(match sat {
        s if s >= BUCKETS[3] => 3,
        s if s >= BUCKETS[2] => 2,
        s if s >= BUCKETS[1] => 1,
        _ => 0,
    })
}

/// A cell's colour: the *dominant* element's hue at its bucket's brightness.
/// Blending was tried and rejected — averaging five hues turns every frontier
/// brown. Dominance + stepped brightness reads as terrain: patches have edges,
/// and a strengthening biome visibly changes level. Under a race lens the hue
/// comes from what the element means to that race.
fn cell_color(w: &World, cell: usize, view: MapView) -> Color32 {
    let (el, sat) = w.terrain.dominant(cell);
    let Some(level) = bucket(sat) else {
        return GROUND;
    };
    let bright: u32 = [80, 130, 185, 245][level];
    let c = view_color(view, el);
    let g = GROUND;
    let mix = |cv: u8, gv: u8| -> u8 {
        ((cv as u32 * bright + gv as u32 * (255 - bright)) / 255) as u8
    };
    Color32::from_rgb(mix(c.r(), g.r()), mix(c.g(), g.g()), mix(c.b(), g.b()))
}

// ----------------------------------------------------------------------
// The frame.
// ----------------------------------------------------------------------

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.advance();

        self.frame += 1;
        if self.frame.is_multiple_of(3) {
            let pop = self.w.population();
            for e in Element::ALL {
                let h = self.history.get_mut(e);
                h.push_back(pop[e]);
                if h.len() > HISTORY {
                    h.pop_front();
                }
            }
        }

        // Keyboard shortcuts that mirror the TUI.
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.paused = !self.paused;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Mode::Body(_) = self.mode {
                self.mode = Mode::Soul;
                self.say("The body wanders on without you.");
            }
        }

        self.top_bar(ctx);
        self.side_panel(ctx);
        self.map_panel(ctx);
        self.death_modal(ctx);

        ctx.request_repaint();
    }
}

impl App {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("CHAOTIC NATURE").strong().size(16.0));
                ui.separator();

                let btn = if self.paused { "▶ resume" } else { "⏸ pause" };
                if ui.button(btn).clicked() {
                    self.paused = !self.paused;
                }

                ui.label("speed");
                ui.add(
                    egui::Slider::new(&mut self.speed, 1.0..=100_000.0)
                        .logarithmic(true)
                        .custom_formatter(|v, _| format!("{:.0} t/s ({:.0}× real)", v, v / 1.667))
                        .custom_parser(|s| s.parse::<f64>().ok()),
                )
                .on_hover_text(
                    "Simulation ticks per second. Real time is 1.67 t/s — one tick per 0.6 s. \
                     Wind it up to watch geology; wind it down to live a life.",
                );

                ui.separator();
                ui.label("view");
                {
                    let all = self.view == MapView::All;
                    if ui.selectable_label(all, "All").on_hover_text("Every resource: each tile's strongest element in its own colour.").clicked() {
                        self.view = MapView::All;
                    }
                    for e in Element::ALL {
                        let sel = self.view == MapView::Race(e);
                        let chip = ui.selectable_label(sel, RichText::new(e.name()).color(ecolor(e)));
                        if chip
                            .on_hover_text(format!(
                                "The world as {} sees it: {} glows green (it feeds you), {} glows red (it hunts you), the rest fade to gray.",
                                e.name(),
                                e.eats().name(),
                                e.eaten_by().name()
                            ))
                            .clicked()
                        {
                            self.view = MapView::Race(e);
                        }
                    }
                }

                ui.separator();
                ui.label("creatures");
                {
                    let mut m = self.t.speed_mult as f64 / 1000.0;
                    let r = ui.add(
                        egui::Slider::new(&mut m, 0.05..=3.0)
                            .logarithmic(true)
                            .custom_formatter(|v, _| format!("×{v:.2}"))
                            .custom_parser(|s| s.parse::<f64>().ok()),
                    )
                    .on_hover_text(
                        "Global movement multiplier for every race at once, on top of the                          per-race speed knobs. Mobility is the ecology axis with a cliff in                          it: fast worlds smear their biomes into one; slow worlds hold                          their ground. Turn this down if wanderers cross the map in a                          fraction of a life.",
                    );
                    if r.changed() {
                        self.t.speed_mult = (m * 1000.0).round().clamp(10.0, 4000.0) as u32;
                        self.retuned = true;
                    }
                }

                ui.separator();
                let min = self.w.tick / TERRAIN_PERIOD;
                ui.label(format!("day {} · {:02}:{:02}", min / 1440, (min / 60) % 24, min % 60))
                    .on_hover_text("Simulated time. One terrain tick — when the ground itself updates — is one simulated minute.");

                ui.separator();
                let pop = self.w.population();
                for e in Element::ALL {
                    ui.label(RichText::new(format!("● {}", pop[e])).color(ecolor(e)))
                        .on_hover_text(format!("{} alive: {}", e.name(), pop[e]));
                }

                if self.retuned {
                    ui.separator();
                    ui.label(RichText::new("✎ retuned").italics())
                        .on_hover_text("Knobs have been changed while this world runs — this run is no longer reproducible from the shipped tables. Restart to get a clean run.");
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((msg, at)) = &self.notice {
                        if at.elapsed().as_secs_f32() < 6.0 {
                            ui.label(RichText::new(msg).strong());
                        } else {
                            self.notice = None;
                        }
                    }
                });
            });
        });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("side")
            .resizable(true)
            .default_width(360.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.mode_card(ui);
                    ui.add_space(8.0);
                    self.events_card(ui);
                    ui.add_space(8.0);
                    self.knobs_card(ui);
                    ui.add_space(8.0);
                    self.population_card(ui);
                    ui.add_space(8.0);
                    self.legend_card(ui);
                });
            });
    }

    fn mode_card(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            match self.mode {
                Mode::Soul => {
                    ui.label(RichText::new("You are a soul").strong().size(15.0));
                    if self.await_claim.is_some() {
                        ui.label("Reaching for the moment…");
                    } else {
                        ui.label(
                            "The world below runs on its own. The glowing ✶ marks are \
                             birth-moments — places the land will take a new body. \
                             Click one on the map, or take one from the list below.",
                        );
                    }
                }
                Mode::Body(id) => match self.w.entities.binary_search_by_key(&id, |e| e.id) {
                    Ok(i) => {
                        let e = &self.w.entities[i];
                        let el = e.element;
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("You are {}", el.name())).strong().size(15.0).color(ecolor(el)));
                            ui.label(format!("· hp {}", e.hp));
                        });
                        let frac = (e.age as f32 / e.lifespan.max(1) as f32).min(1.0);
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .fill(ecolor(el))
                                .text(format!("{} of {} lived", duration(e.age), duration(e.lifespan))),
                        )
                        .on_hover_text("A body ages at its race's tempo and dies of old age. Death is not failure — the corpse terraforms the cell it falls on.");

                        let a = self.t.races[el];
                        let efrac = (e.energy as f32 / a.repro_threshold.max(1) as f32).min(1.0);
                        ui.add(
                            egui::ProgressBar::new(efrac)
                                .fill(Color32::from_rgb(235, 200, 90))
                                .text(format!("energy {} — offspring at {}", e.energy, a.repro_threshold)),
                        )
                        .on_hover_text(format!(
                            "You eat {} ground where you stand — a meal every {} ticks, bounded by what the cell holds. \
                             Fill this bar and your body splits off another {}.",
                            el.eats().name(),
                            pentagram::race::RaceAttrs::FEED_PERIOD,
                            el.name(),
                        ));
                        if e.energy == 0 {
                            ui.label(
                                RichText::new(format!(
                                    "STARVING — stand on {} ground to eat",
                                    el.eats().name()
                                ))
                                .color(Color32::from_rgb(230, 90, 70))
                                .strong(),
                            );
                        }

                        ui.label("Click the ground to move toward it. esc releases the body — it lives on as wildlife.");
                        if ui.button("Release the body").clicked() {
                            self.mode = Mode::Soul;
                            self.say("The body wanders on without you.");
                        }
                    }
                    Err(_) => {
                        ui.label("between bodies…");
                    }
                },
            }
        });
    }

    fn events_card(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Birth-moments").strong());
        let evs: Vec<Event> = {
            let mut v = self.w.events.clone();
            v.sort_by_key(|e| (e.closes, e.id));
            v
        };
        if evs.is_empty() {
            ui.label(RichText::new("None open — the world is between moments. Terrain past its threshold opens one; wild sparks strike on their own.").weak());
        }
        let side = self.w.size.floor_int().max(1) as u32;
        for ev in evs.iter().take(8) {
            let el = ev.element;
            let left = ev.closes.saturating_sub(self.w.tick);
            let window = ev.closes.saturating_sub(ev.opened).max(1);
            let frac = (left as f32 / window as f32).clamp(0.0, 1.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("✶").color(ecolor(el)).size(15.0));
                ui.label(RichText::new(el.name()).color(ecolor(el)).strong());
                ui.label(format!("({}, {})", ev.cell % side, ev.cell / side));
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(80.0)
                        .fill(ecolor(el))
                        .text(duration(left)),
                )
                .on_hover_text("Time left in this moment's window. Unclaimed moments are spent by the world itself as wildlife births.");
                let can = matches!(self.mode, Mode::Soul) && self.await_claim.is_none();
                if ui.add_enabled(can, egui::Button::new("incarnate")).clicked() {
                    self.pending.push((0, CmdKind::Incarnate { event: ev.id }));
                    self.await_claim = Some(ev.id);
                    if self.paused {
                        self.say(format!("Reaching for the {} moment — resume to be born.", el.name()));
                    } else {
                        self.say(format!("Reaching for the {} moment…", el.name()));
                    }
                }
            });
        }
    }

    fn population_card(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Population").strong());
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 72.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 3.0, GROUND);
        let peak = Element::ALL
            .iter()
            .flat_map(|e| self.history[*e].iter().copied())
            .max()
            .unwrap_or(1)
            .max(1) as f32;
        for e in Element::ALL {
            let h = &self.history[e];
            if h.len() < 2 {
                continue;
            }
            let pts: Vec<Pos2> = h
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let x = rect.left() + i as f32 / (HISTORY - 1) as f32 * rect.width();
                    let y = rect.bottom() - (*v as f32 / peak) * (rect.height() - 4.0) - 2.0;
                    Pos2::new(x, y)
                })
                .collect();
            p.add(egui::Shape::line(pts, Stroke::new(1.5_f32, ecolor(e))));
        }
    }

    fn knobs_card(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(
            RichText::new("Tuning — speed, lifespan, deposition, consumption…").strong(),
        )
            .default_open(true)
            .show(ui, |ui| {
                ui.label(RichText::new("Pick a race, drag a slider — the running world updates. Hover any slider for what it does.").weak());

                egui::CollapsingHeader::new("Master ratios — all five races at once")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Bump a whole axis up or down; the ratios between races are preserved.").weak());
                        let mut touched = false;
                        for m in MASTERS {
                            ui.horizontal(|ui| {
                                for (label, n, d) in [("÷2", 1u64, 2u64), ("÷1.25", 4, 5), ("×1.25", 5, 4), ("×2", 2, 1)] {
                                    if ui.small_button(label).clicked() {
                                        (m.apply)(&mut self.t, n, d);
                                        touched = true;
                                    }
                                }
                                ui.label(m.name).on_hover_text(m.help);
                            });
                        }
                        if touched {
                            self.retuned = true;
                        }
                    });
                ui.separator();

                ui.horizontal(|ui| {
                    for e in Element::ALL {
                        let sel = self.knob_element == e;
                        if ui
                            .selectable_label(sel, RichText::new(e.name()).color(ecolor(e)))
                            .clicked()
                        {
                            self.knob_element = e;
                        }
                    }
                });
                ui.separator();

                let e = self.knob_element;
                for page in PAGES {
                    egui::CollapsingHeader::new(page.title)
                        .default_open(page.title.starts_with("body"))
                        .show(ui, |ui| {
                            for k in page.knobs {
                                knob_slider(ui, k, &mut self.t, e, &mut self.retuned);
                            }
                        });
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("wander steering");
                    ui.add(egui::Slider::new(&mut self.wander_pct, 0..=50).suffix("%"))
                        .on_hover_text("Share of wildlife the view re-steers each second, as ordinary input commands. Without it bodies bounce between walls on rails until real behaviour arrives at S2.");
                });

                ui.horizontal(|ui| {
                    if ui.button("Restart world").on_hover_text("Same seed, tick 0, current knobs — a reproducible run.").clicked() {
                        self.restart(self.seed);
                        self.retuned = false;
                        self.say("Restarted at tick 0 — this run is reproducible from these knobs.");
                    }
                    if ui.button("New world").on_hover_text("A different seed: new climate geography, same knobs.").clicked() {
                        let seed = self.seed.wrapping_add(1);
                        self.restart(seed);
                        self.say(format!("New world — seed {seed:#x}."));
                    }
                    if ui.button("Write tuned table").on_hover_text("Dump the current tables to src/race.tuned.rs to copy back into the source.").clicked() {
                        match write_table(&self.t) {
                            Ok(p) => self.say(format!("Wrote {p}")),
                            Err(e) => self.say(format!("Could not write the table: {e}")),
                        }
                    }
                });
            });
    }

    fn legend_card(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("What am I looking at?").strong())
            .default_open(self.show_help)
            .show(ui, |ui| {
                self.show_help = false;
                ui.label("Each tile shows its strongest element at one of four strength levels — the land remembering what lived and died on it. There is no geography: biomes are corpse-piles and habits. Where a race lives is where its element grows, and its element is somebody's food.");
                ui.horizontal_wrapped(|ui| {
                    for e in Element::ALL {
                        ui.label(RichText::new(format!("■ {}", e.name())).color(ecolor(e)));
                    }
                });
                ui.add_space(4.0);
                ui.label("• dots — living creatures, coloured by race");
                ui.label("• ✶ — birth-moments: click one to be born there");
                ui.label("• hover any ground for its exact saturations");
                ui.label("• the view chips up top re-colour the whole map through one race's eyes: green feeds it, red hunts it, gray is neither");
                ui.add_space(4.0);
                ui.label(RichText::new(
                    "The cycle: everything eats its neighbour — Wood→Fire→Earth→Metal→Water→Wood. \
                     Corpses feed the ground; saturated ground opens birth-moments; unclaimed \
                     moments become wildlife. The circle of life is the whole game.",
                ).weak());
            });
    }

    // ------------------------------------------------------------------
    // The map.
    // ------------------------------------------------------------------

    fn map_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(Color32::from_rgb(10, 11, 15)))
            .show(ctx, |ui| {
                let side = self.w.size.floor_int().max(1) as usize;

                // Terrain texture, rebuilt every frame — 9 216 pixels is free.
                let mut img = egui::ColorImage::new([side, side], GROUND);
                for (i, px) in img.pixels.iter_mut().enumerate() {
                    *px = cell_color(&self.w, i, self.view);
                }
                // NEAREST, not LINEAR: cells are tiles, and tiles have edges.
                // Linear filtering turned the whole field into fog.
                let opts = egui::TextureOptions::NEAREST;
                match &mut self.terrain_tex {
                    Some(t) => t.set(img, opts),
                    None => self.terrain_tex = Some(ctx.load_texture("terrain", img, opts)),
                }

                let avail = ui.available_rect_before_wrap();
                let dim = avail.width().min(avail.height());
                let rect = Rect::from_center_size(avail.center(), Vec2::splat(dim));
                let (resp, painter) = ui.allocate_painter(avail.size(), Sense::click_and_drag());

                if let Some(tex) = &self.terrain_tex {
                    painter.image(
                        tex.id(),
                        rect,
                        Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, Color32::from_gray(60)));

                let scale = rect.width() / side as f32;
                let to_screen = |x: f32, y: f32| -> Pos2 {
                    Pos2::new(rect.left() + x * scale, rect.top() + y * scale)
                };
                let to_world = |p: Pos2| -> (f32, f32) {
                    ((p.x - rect.left()) / scale, (p.y - rect.top()) / scale)
                };

                // Creatures.
                let player_id = match self.mode {
                    Mode::Body(id) => Some(id),
                    Mode::Soul => None,
                };
                for ent in &self.w.entities {
                    if !ent.alive {
                        continue;
                    }
                    let p = to_screen(ent.pos.x.to_f32_render(), ent.pos.y.to_f32_render());
                    let r = (self.t.races[ent.element].radius.to_f32_render() * scale * 0.6).clamp(1.5, 8.0);
                    painter.circle_filled(p, r, view_color(self.view, ent.element));
                    if player_id == Some(ent.id) {
                        painter.circle_stroke(p, r + 4.0, Stroke::new(2.0_f32, Color32::WHITE));
                        painter.text(
                            p + Vec2::new(0.0, -(r + 16.0)),
                            egui::Align2::CENTER_CENTER,
                            "YOU",
                            egui::FontId::proportional(12.0),
                            Color32::WHITE,
                        );
                    }
                }

                // Birth-moments: pulsing rings.
                let pulse = (ui.input(|i| i.time) * 3.0).sin() as f32 * 0.5 + 0.5;
                let mut ev_screen: Vec<(Pos2, Event)> = Vec::new();
                for ev in &self.w.events {
                    let ex = (ev.cell as usize % side) as f32 + 0.5;
                    let ey = (ev.cell as usize / side) as f32 + 0.5;
                    let p = to_screen(ex, ey);
                    let c = view_color(self.view, ev.element);
                    painter.circle_stroke(p, 6.0 + pulse * 4.0, Stroke::new(2.0_f32, c));
                    painter.circle_filled(p, 3.0, Color32::WHITE.gamma_multiply(0.6 + 0.4 * pulse));
                    painter.circle_filled(p, 2.0, c);
                    ev_screen.push((p, *ev));
                }

                // Interaction.
                if let Some(hp) = resp.hover_pos() {
                    if rect.contains(hp) {
                        let near = ev_screen
                            .iter()
                            .map(|(p, ev)| (p.distance(hp), *ev))
                            .filter(|(d, _)| *d < 16.0)
                            .min_by(|a, b| a.0.total_cmp(&b.0));

                        if let Some((_, ev)) = near {
                            let el = ev.element;
                            let left = duration(ev.closes.saturating_sub(self.w.tick));
                            let act = if matches!(self.mode, Mode::Soul) {
                                "Click to incarnate here."
                            } else {
                                "A soul could be born here — you already have a body."
                            };
                            egui::show_tooltip_at_pointer(
                                ctx,
                                ui.layer_id(),
                                egui::Id::new("evtip"),
                                |ui| {
                                    ui.label(RichText::new(format!("✶ {} birth-moment", el.name())).color(ecolor(el)).strong());
                                    ui.label(format!("closes in {left}"));
                                    ui.label(RichText::new(act).weak());
                                },
                            );
                        } else {
                            let (wx, wy) = to_world(hp);
                            let cx = (wx as usize).min(side - 1);
                            let cy = (wy as usize).min(side - 1);
                            let cell = cy * side + cx;
                            egui::show_tooltip_at_pointer(
                                ctx,
                                ui.layer_id(),
                                egui::Id::new("celltip"),
                                |ui| {
                                    ui.label(RichText::new(format!("ground at ({cx}, {cy})")).weak());
                                    for e in Element::ALL {
                                        let s = self.w.terrain.sat[e.index()][cell];
                                        let frac = s as f32 / 65_535.0;
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(format!("{:<6}", e.name())).color(ecolor(e)));
                                            let (r, _) = ui.allocate_exact_size(Vec2::new(80.0, 8.0), Sense::hover());
                                            let p = ui.painter_at(r);
                                            p.rect_filled(r, 2.0, Color32::from_gray(35));
                                            let mut fr = r;
                                            fr.set_width(r.width() * frac.max(0.01));
                                            p.rect_filled(fr, 2.0, ecolor(e));
                                            ui.label(format!("{s}"));
                                        });
                                    }
                                    if matches!(self.mode, Mode::Body(_)) {
                                        ui.label(RichText::new("Click to move toward here.").weak());
                                    }
                                },
                            );
                        }

                        // Clicks.
                        match self.mode {
                            Mode::Soul => {
                                if resp.clicked() && self.await_claim.is_none() {
                                    if let Some((_, ev)) = near {
                                        self.pending.push((0, CmdKind::Incarnate { event: ev.id }));
                                        self.await_claim = Some(ev.id);
                                        let msg = if self.paused {
                                            format!("Reaching for the {} moment — resume to be born.", ev.element.name())
                                        } else {
                                            format!("Reaching for the {} moment…", ev.element.name())
                                        };
                                        self.say(msg);
                                    }
                                }
                            }
                            Mode::Body(id) => {
                                let steer = resp.clicked()
                                    || (resp.dragged() && self.last_steer.elapsed().as_millis() > 150);
                                if steer {
                                    self.last_steer = Instant::now();
                                    if let Ok(i) = self.w.entities.binary_search_by_key(&id, |e| e.id) {
                                        let ent = &self.w.entities[i];
                                        let (wx, wy) = to_world(hp);
                                        let dx = wx - ent.pos.x.to_f32_render();
                                        let dy = wy - ent.pos.y.to_f32_render();
                                        if dx.abs() > 0.2 || dy.abs() > 0.2 {
                                            self.pending.push((
                                                id,
                                                CmdKind::SetHeading {
                                                    dir: V2::new(
                                                        Fx::ratio((dx * 100.0) as i32, 100),
                                                        Fx::ratio((dy * 100.0) as i32, 100),
                                                    ),
                                                },
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            });
    }

    fn death_modal(&mut self, ctx: &egui::Context) {
        let mut clear = false;
        if let Some(text) = &self.obituary {
            egui::Window::new("The land takes back its own")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(text);
                    ui.add_space(6.0);
                    if ui.button("Return as a soul").clicked() {
                        clear = true;
                    }
                });
        }
        if clear {
            self.obituary = None;
        }
    }
}

/// A master ratio control: one axis, scaled across all five races at once by
/// `num / den`, ratios preserved. Buttons rather than sliders so there is no
/// base value to track — each press is a plain multiplicative nudge.
struct Master {
    name: &'static str,
    help: &'static str,
    apply: fn(&mut Tuning, u64, u64),
}

fn scale_u64(v: &mut u64, n: u64, d: u64, lo: u64, hi: u64) {
    *v = (v.saturating_mul(n) / d.max(1)).clamp(lo, hi);
}

fn scale_u32(v: &mut u32, n: u64, d: u64, lo: u32, hi: u32) {
    *v = ((*v as u64).saturating_mul(n) / d.max(1)).clamp(lo as u64, hi as u64) as u32;
}

fn scale_u16(v: &mut u16, n: u64, d: u64, lo: u16, hi: u16) {
    *v = ((*v as u64).saturating_mul(n) / d.max(1)).clamp(lo as u64, hi as u64) as u16;
}

const MASTERS: &[Master] = &[
    Master {
        name: "lifespan",
        help: "How long every body lives. Down: the whole world turns over faster.",
        apply: |t, n, d| for e in Element::ALL {
            scale_u64(&mut t.races[e].lifespan, n, d, 100, 7_776_000);
        },
    },
    Master {
        name: "movement speed",
        help: "The ecology axis with a cliff in it: enough mobility and biomes stop coexisting.",
        apply: |t, n, d| for e in Element::ALL {
            let raw = t.races[e].speed.raw() as i64;
            let scaled = (raw.saturating_mul(n as i64) / d.max(1) as i64).clamp(65, 4 * 65_536);
            t.races[e].speed = Fx::from_raw(scaled as i32);
        },
    },
    Master {
        name: "deposition",
        help: "How hard every race writes itself into the terrain over a life.",
        apply: |t, n, d| for e in Element::ALL {
            scale_u64(&mut t.races[e].deposit_unit, n, d, 1, 100_000_000);
        },
    },
    Master {
        name: "consumption",
        help: "The ambient drain every race takes from the terrain over a life.",
        apply: |t, n, d| for e in Element::ALL {
            scale_u64(&mut t.races[e].consume_unit, n, d, 1, 100_000_000);
        },
    },
    Master {
        name: "hunger (upkeep)",
        help: "Energy burned per tick by being alive. Up: leaner world, more starvation.",
        apply: |t, n, d| for e in Element::ALL {
            scale_u32(&mut t.races[e].upkeep, n, d, 0, 1_000);
        },
    },
    Master {
        name: "bite size",
        help: "How much one meal can strip from the cell underfoot.",
        apply: |t, n, d| for e in Element::ALL {
            scale_u32(&mut t.races[e].bite, n, d, 1, 100_000);
        },
    },
    Master {
        name: "fertility",
        help: "Up: breeding needs less surplus, populations run away sooner. (Scales the breeding threshold and cost down.)",
        apply: |t, n, d| for e in Element::ALL {
            // Inverse: more fertility = lower bars. Cost keeps its floor above
            // birth energy so the table stays valid.
            let floor_cost = t.races[e].birth_energy.max(1);
            scale_u32(&mut t.races[e].repro_cost, d, n, floor_cost, 200_000);
            let floor_thr = t.races[e].repro_cost;
            scale_u32(&mut t.races[e].repro_threshold, d, n, floor_thr, 200_000);
        },
    },
    Master {
        name: "terrain decay",
        help: "How fast the land forgets. Down: scars last for days.",
        apply: |t, n, d| for e in Element::ALL {
            scale_u16(&mut t.terrain[e].decay, n, d, 0, 1_000);
        },
    },
    Master {
        name: "wild sparks",
        help: "How often lightning offers a way in, anywhere, regardless of terrain.",
        apply: |t, n, d| for e in Element::ALL {
            scale_u16(&mut t.terrain[e].wild, n, d, 0, 1_000);
        },
    },
];

/// One knob as a slider, driven entirely by the shared tuning table — the
/// name, range, logarithmic feel and hover help all come from the same row
/// the terminal client uses.
fn knob_slider(ui: &mut egui::Ui, k: &Knob, t: &mut Tuning, e: Element, retuned: &mut bool) {
    let mut v = k.value(t, e);
    let log = matches!(k.step, pentagram::tuning::Step::Scale) && k.lo >= 0 && k.hi > 1000;
    let resp = ui.add(
        egui::Slider::new(&mut v, k.lo.max(if log { 1 } else { k.lo })..=k.hi)
            .logarithmic(log)
            .custom_formatter(move |x, _| k.short(x as i64))
            .custom_parser(|s| s.parse::<f64>().ok())
            .text(k.name),
    );
    let resp = resp.on_hover_text(k.help);
    if resp.changed() {
        (k.set)(t, e, v);
        *retuned = true;
    }
}
