//! The tuning table, as a thing you can point at.
//!
//! Every knob is one row in [`PAGES`]: a name, how to format it, how big a
//! nudge is, its safe range, and a getter/setter pair. Adding a knob to the
//! live view means adding a row here and nothing else — the grid, the cursor,
//! the stepping and the detail line all fall out of this table.
//!
//! Ranges are not decoration. `deposit_unit` feeds `demand`, which is summed in
//! milli-units over a terrain period; the crate builds with `overflow-checks`
//! in every profile, so a knob with no ceiling is a panic waiting for a curious
//! user. Every `hi` here is chosen to keep the worst case inside `u64`.

use pentagram::element::{Element, PerElement};
use pentagram::fx::Fx;
use pentagram::race::{Channel, Edge, RaceAttrs, TICKS_PER_DAY, TICKS_PER_MINUTE, RACES};
use pentagram::terrain::{TerrainAttrs, TERRAIN};

/// Everything the live view can change, in one struct.
///
/// `races` and `terrain` are the real tuning tables and go straight into the
/// world. `restock` is the *view's* knob, not the simulation's: without a hand
/// on the tiller a young world empties out. It is applied as ordinary input
/// commands, which is exactly what a player would be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tuning {
    pub races: PerElement<RaceAttrs>,
    pub terrain: PerElement<TerrainAttrs>,
    pub restock: PerElement<u32>,
}

impl Tuning {
    pub fn new(per_race: u32) -> Tuning {
        Tuning {
            races: RACES,
            terrain: TERRAIN,
            restock: PerElement::filled(per_race),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    /// Plain count, abbreviated in the grid and exact in the detail line.
    Int,
    /// Sim ticks, shown as the duration they actually are.
    Ticks,
    /// Per-mille of a whole.
    Permille,
    /// Hundredths of a cell.
    Cells,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// Fixed increment. Coarse adjust is ten of them.
    Add(i64),
    /// Proportional: about 10% a press, doubling on coarse adjust. The only
    /// workable choice for knobs that span four orders of magnitude, like
    /// lifespan running from eight minutes to a fortnight.
    Scale,
}

pub struct Knob {
    pub name: &'static str,
    pub help: &'static str,
    pub fmt: Fmt,
    pub step: Step,
    pub lo: i64,
    pub hi: i64,
    pub get: fn(&Tuning, Element) -> i64,
    pub set: fn(&mut Tuning, Element, i64),
}

impl Knob {
    #[inline]
    pub fn value(&self, t: &Tuning, e: Element) -> i64 {
        (self.get)(t, e)
    }

    pub fn nudge(&self, t: &mut Tuning, e: Element, up: bool, coarse: bool) {
        let v = self.value(t, e);
        let next = match self.step {
            Step::Add(n) => {
                let n = if coarse { n * 10 } else { n };
                if up {
                    v.saturating_add(n)
                } else {
                    v.saturating_sub(n)
                }
            }
            Step::Scale if coarse => {
                if up {
                    v.saturating_mul(2).max(v + 1)
                } else {
                    v / 2
                }
            }
            Step::Scale => {
                if up {
                    v.saturating_add((v / 10).max(1))
                } else {
                    v.saturating_sub((v / 11).max(1))
                }
            }
        };
        (self.set)(t, e, next.clamp(self.lo, self.hi));
    }

    /// Compact form, for a grid cell.
    pub fn short(&self, v: i64) -> String {
        match self.fmt {
            Fmt::Int => abbrev(v),
            Fmt::Ticks => duration(v.max(0) as u64),
            Fmt::Permille => format!("{v}"),
            Fmt::Cells => format!("{}.{:02}", v / 100, (v % 100).abs()),
        }
    }

    /// Exact form, for the detail line under the grid.
    pub fn long(&self, v: i64) -> String {
        match self.fmt {
            Fmt::Int => grouped(v),
            Fmt::Ticks => format!("{} ticks · {}", grouped(v), duration(v.max(0) as u64)),
            Fmt::Permille => format!("{v}‰ of the unit"),
            Fmt::Cells => format!("{}.{:02} cells", v / 100, (v % 100).abs()),
        }
    }
}

pub struct Page {
    pub title: &'static str,
    pub knobs: &'static [Knob],
}

macro_rules! knob {
    ($name:literal, $fmt:expr, $step:expr, $lo:expr, $hi:expr, $help:literal,
     |$t:ident, $e:ident| $get:expr,
     |$tm:ident, $em:ident, $v:ident| $set:expr) => {
        Knob {
            name: $name,
            help: $help,
            fmt: $fmt,
            step: $step,
            lo: $lo,
            hi: $hi,
            get: |$t: &Tuning, $e: Element| -> i64 { $get },
            set: |$tm: &mut Tuning, $em: Element, $v: i64| { $set },
        }
    };
}

/// A band edge. Deposit and consume are the same three edges twice over, and
/// each one has to go through `set_edge` so the band cannot be left inverted.
macro_rules! edge_knob {
    ($name:literal, $field:ident, $read:ident, $edge:expr, $help:literal) => {
        knob!($name, Fmt::Int, Step::Scale, 0, 1_000_000, $help,
              |t, e| t.races[e].$field.$read as i64,
              |t, e, v| t.races[e].$field.set_edge($edge, v as u32))
    };
}

macro_rules! burst_knob {
    ($name:literal, $field:ident, $help:literal) => {
        knob!($name, Fmt::Int, Step::Add(1), 1, 500, $help,
              |t, e| t.races[e].$field.burst_ticks as i64,
              |t, e, v| t.races[e].$field.burst_ticks = v as u32)
    };
}

static BODY: [Knob; 15] = [
    knob!("lifespan", Fmt::Ticks, Step::Scale, 100, TICKS_PER_DAY as i64 * 90,
          "how long one body persists before it expires of old age",
          |t, e| t.races[e].lifespan as i64,
          |t, e, v| t.races[e].lifespan = v as u64),
    knob!("life variance", Fmt::Permille, Step::Add(10), 0, 900,
          "per-mille spread on lifespan, so a cohort born together does not die together",
          |t, e| t.races[e].lifespan_variance as i64,
          |t, e, v| t.races[e].lifespan_variance = v as u16),
    knob!("speed", Fmt::Cells, Step::Add(1), 0, 400,
          "cells per tick — an ECOLOGY knob: it decides whether biomes coexist or collapse to one",
          |t, e| (t.races[e].speed.raw() as i64 * 100 + 32_768) / 65_536,
          |t, e, v| t.races[e].speed = Fx::ratio(v as i32, 100)),
    knob!("radius", Fmt::Cells, Step::Add(5), 1, 2_000,
          "collision radius in cells; bigger bodies crowd each other out of a region sooner",
          |t, e| (t.races[e].radius.raw() as i64 * 100 + 32_768) / 65_536,
          |t, e, v| t.races[e].radius = Fx::ratio(v as i32, 100)),
    knob!("restock to", Fmt::Int, Step::Add(5), 0, 250,
          "VIEW KNOB: bodies of this race the view keeps spawning back, as ordinary input commands",
          |t, e| t.restock[e] as i64,
          |t, e, v| t.restock[e] = v as u32),

    knob!("deposit unit", Fmt::Int, Step::Scale, 1, 100_000_000,
          "total a body writes to the terrain over its ENTIRE life, split across the channels",
          |t, e| t.races[e].deposit_unit as i64,
          |t, e, v| t.races[e].deposit_unit = v as u64),
    edge_knob!("dep floor", deposit, floor, Edge::Floor,
               "granted every terrain tick even at zero demand — the world's own churn"),
    edge_knob!("dep nominal", deposit, nominal, Edge::Nominal,
               "long-run average under sustained demand; the burst bucket refills at this rate"),
    edge_knob!("dep ceiling", deposit, ceiling, Edge::Ceiling,
               "never exceeded in one terrain tick, under any behaviour whatsoever"),
    burst_knob!("dep burst", deposit,
                "terrain ticks of nominal that can be banked, then spent all at once"),

    knob!("consume unit", Fmt::Int, Step::Scale, 1, 100_000_000,
          "total a body takes from the terrain over its entire life",
          |t, e| t.races[e].consume_unit as i64,
          |t, e, v| t.races[e].consume_unit = v as u64),
    edge_knob!("con floor", consume, floor, Edge::Floor,
               "taken every terrain tick even at zero demand"),
    edge_knob!("con nominal", consume, nominal, Edge::Nominal,
               "long-run average consumption under sustained demand"),
    edge_knob!("con ceiling", consume, ceiling, Edge::Ceiling,
               "never exceeded in one terrain tick"),
    burst_knob!("con burst", consume,
                "terrain ticks of nominal consumption that can be banked"),
];

/// One row per channel, twice. The mix is what makes two races with identical
/// rates feel nothing alike, so it gets its own page rather than being buried.
macro_rules! mix_knobs {
    ($prefix:literal, $field:ident, $chan:expr, $help:literal) => {
        knob!($prefix, Fmt::Permille, Step::Add(25), 0, 1000, $help,
              |t, e| t.races[e].$field.permille($chan) as i64,
              |t, e, v| t.races[e].$field.set_rebalanced($chan, v as u16))
    };
}

static MIX: [Knob; 10] = [
    mix_knobs!("dep birth", deposit_mix, Channel::OnBirth,
               "written at the moment of incarnation — fires once per life"),
    mix_knobs!("dep death", deposit_mix, Channel::OnDeath,
               "written by the corpse — fires once per life; dominant for short-lived races"),
    mix_knobs!("dep action", deposit_mix, Channel::OnAction,
               "written by moving — fires every tick a body actually travels"),
    mix_knobs!("dep consume", deposit_mix, Channel::OnConsume,
               "written at the moment of refining what was eaten — one meal per 200 ticks"),
    mix_knobs!("dep existence", deposit_mix, Channel::OnExistence,
               "written by merely being here — once per body per terrain tick"),
    mix_knobs!("con birth", consume_mix, Channel::OnBirth, "taken at incarnation"),
    mix_knobs!("con death", consume_mix, Channel::OnDeath, "taken by the corpse"),
    mix_knobs!("con action", consume_mix, Channel::OnAction, "taken by moving"),
    mix_knobs!("con consume", consume_mix, Channel::OnConsume, "taken by feeding"),
    mix_knobs!("con existence", consume_mix, Channel::OnExistence, "taken by being present"),
];

static TERRAIN_KNOBS: [Knob; 9] = [
    knob!("influx", Fmt::Int, Step::Scale, 0, 100_000,
          "saturation seeping in per terrain tick at each climate hotspot — how strong this biome's geography is",
          |t, e| t.terrain[e].influx as i64,
          |t, e, v| t.terrain[e].influx = v as u32),
    knob!("decay ‰", Fmt::Permille, Step::Add(1), 0, 1000,
          "passive forgetting per terrain tick; low values make scars persist for days",
          |t, e| t.terrain[e].decay as i64,
          |t, e, v| t.terrain[e].decay = v as u16),
    knob!("generate ‰", Fmt::Permille, Step::Add(1), 0, 1000,
          "ring maturation: how fast this element becomes the next — THE succession speed knob",
          |t, e| t.terrain[e].generate as i64,
          |t, e, v| t.terrain[e].generate = v as u16),
    knob!("overcome ‰", Fmt::Permille, Step::Add(1), 0, 1000,
          "star erosion: how hard this element's presence erodes the element it suppresses",
          |t, e| t.terrain[e].overcome as i64,
          |t, e, v| t.terrain[e].overcome = v as u16),
    knob!("diffuse ‰", Fmt::Permille, Step::Add(10), 0, 1000,
          "share spread to the four neighbours per terrain tick — reach is hard-capped at one cell",
          |t, e| t.terrain[e].diffuse as i64,
          |t, e, v| t.terrain[e].diffuse = v as u16),
    knob!("event gate", Fmt::Int, Step::Scale, 0, 65_535,
          "minimum saturation for this element to host a terrain-gated incarnation event",
          |t, e| t.terrain[e].ev_threshold as i64,
          |t, e, v| t.terrain[e].ev_threshold = v as u16),
    knob!("event window", Fmt::Ticks, Step::Scale, 50, 1_000_000,
          "how long an incarnation event stays open — Fire's moment is brief, Earth's builds for ages",
          |t, e| t.terrain[e].ev_window as i64,
          |t, e, v| t.terrain[e].ev_window = v as u32),
    knob!("wild ‰", Fmt::Permille, Step::Add(5), 0, 1000,
          "chance per terrain tick of a wild event anywhere — the lightning strike; the floor under extinction",
          |t, e| t.terrain[e].wild as i64,
          |t, e, v| t.terrain[e].wild = v as u16),
    knob!("wild cap", Fmt::Int, Step::Add(5), 0, 500,
          "wildlife bodies the world sustains from unclaimed events; expiry only births below this",
          |t, e| t.terrain[e].wild_cap as i64,
          |t, e, v| t.terrain[e].wild_cap = v as u32),
];

pub static PAGES: &[Page] = &[
    Page { title: "body & rates", knobs: &BODY },
    Page { title: "channel mix ‰  (edits rebalance the rest to keep the sum at 1000)", knobs: &MIX },
    Page { title: "terrain & incarnation", knobs: &TERRAIN_KNOBS },
];

// ----------------------------------------------------------------------
// Formatting.
// ----------------------------------------------------------------------

/// Sim ticks as the duration they represent. 100 ticks is one simulated minute.
pub fn duration(ticks: u64) -> String {
    let m = ticks / TICKS_PER_MINUTE;
    if m < 1 {
        return format!("{ticks}t");
    }
    if m < 90 {
        return format!("{m}m");
    }
    let h = m / 60;
    if h < 48 {
        return format!("{h}h{:02}", m % 60);
    }
    format!("{}d{:02}h", h / 24, h % 24)
}

/// Three significant-ish figures, so a 12-character column can hold anything
/// from a per-mille to five million.
pub fn abbrev(v: i64) -> String {
    let (sign, a) = if v < 0 { ("-", -v) } else { ("", v) };
    if a < 10_000 {
        format!("{sign}{a}")
    } else if a < 1_000_000 {
        format!("{sign}{}.{}k", a / 1000, (a % 1000) / 100)
    } else {
        format!("{sign}{}.{}M", a / 1_000_000, (a % 1_000_000) / 100_000)
    }
}

/// Digit-grouped with thin spaces, matching how the design doc writes numbers.
pub fn grouped(v: i64) -> String {
    let (sign, a) = if v < 0 { ("-", -v) } else { ("", v) };
    let digits = a.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    format!("{sign}{out}")
}
