//! S1 — the terrain field.
//!
//! Each cell holds five saturations, one per element. Bodies write into the
//! field through the deposition channels and take from it through consumption;
//! the field then churns on its own: saturation matures around the generating
//! ring, erodes along the overcoming star, seeps in from climate hotspots,
//! decays, and diffuses.
//!
//! Succession is not a system anyone writes — it is the generating cycle
//! running on the field. A burnt region deposits Fire, which matures to Earth,
//! then Metal, then Water, then Wood, and burns again.
//!
//! ## Operator order is a wire format
//!
//! Per terrain tick, in this order: **deposit → consume → overcome → generate
//! → influx → decay → diffuse**. Within each operator, elements in ring order,
//! cells in row-major order, updates in place. Reordering any of that changes
//! results and invalidates every recorded replay.
//!
//! ## Invariant I lives here
//!
//! Diffusion moves saturation at most one cell per terrain tick, by
//! construction — the stencil only ever reaches its four neighbours. That
//! bound is what keeps the overlap band between territories a finite number
//! at S3. Do not add an operator that acts at a distance.

use crate::element::{Element, PerElement};
use crate::fx::V2;
use crate::hash::{Hashable, Hasher};
use crate::rand::{rand_below, rand_chance, Channel};

/// Everything tunable about how one element behaves as terrain.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TerrainAttrs {
    /// Saturation injected per terrain tick at each climate hotspot.
    pub influx: u32,
    /// Passive forgetting, per-mille per terrain tick. Low values make scars
    /// persist for days.
    pub decay: u16,
    /// Ring maturation, per-mille per terrain tick: how fast this element
    /// becomes the next one. The succession speed knob.
    pub generate: u16,
    /// Star erosion, per-mille: how hard this element's presence erodes the
    /// element it suppresses, in the same cell.
    pub overcome: u16,
    /// Share spread to the four neighbours, per-mille per terrain tick.
    /// Capped at one cell of reach by construction.
    pub diffuse: u16,
    /// Minimum saturation for this element to host a terrain-gated
    /// incarnation event.
    pub ev_threshold: u16,
    /// Sim ticks an incarnation event stays open before it expires.
    pub ev_window: u32,
    /// Per-mille chance per terrain tick of a wild event — the lightning
    /// strike. Exists so a race with no viable terrain anywhere is still
    /// joinable; this is the floor under extinction.
    pub wild: u16,
    /// Wildlife bodies of this race the world will sustain from unclaimed
    /// events. Expiry only births a body below this population.
    pub wild_cap: u32,
}

impl TerrainAttrs {
    pub fn is_valid(&self) -> bool {
        self.decay <= 1000
            && self.generate <= 1000
            && self.overcome <= 1000
            && self.diffuse <= 1000
            && self.wild <= 1000
            && self.ev_window > 0
    }
}

impl Hashable for TerrainAttrs {
    fn hash_into(&self, h: &mut Hasher) {
        h.u32(self.influx)
            .u16(self.decay)
            .u16(self.generate)
            .u16(self.overcome)
            .u16(self.diffuse)
            .u16(self.ev_threshold)
            .u32(self.ev_window)
            .u16(self.wild)
            .u32(self.wild_cap);
    }
}

/// Defaults, tuned for a world that becomes legible within a couple of
/// simulated hours. Event windows follow the tempo axis: Fire's moment is
/// brief and unreservable, Earth's builds for half a simulated hour.
pub const TERRAIN: PerElement<TerrainAttrs> = PerElement([
    // Wood
    TerrainAttrs { influx: 380, decay: 8, generate: 14, overcome: 6, diffuse: 200, ev_threshold: 9000, ev_window: 800, wild: 125, wild_cap: 80 },
    // Fire
    TerrainAttrs { influx: 320, decay: 14, generate: 22, overcome: 9, diffuse: 260, ev_threshold: 9000, ev_window: 200, wild: 125, wild_cap: 80 },
    // Earth
    TerrainAttrs { influx: 420, decay: 5, generate: 9, overcome: 6, diffuse: 120, ev_threshold: 9000, ev_window: 3000, wild: 125, wild_cap: 80 },
    // Metal
    TerrainAttrs { influx: 360, decay: 6, generate: 11, overcome: 7, diffuse: 140, ev_threshold: 9000, ev_window: 1500, wild: 125, wild_cap: 80 },
    // Water
    TerrainAttrs { influx: 400, decay: 9, generate: 16, overcome: 8, diffuse: 300, ev_threshold: 9000, ev_window: 400, wild: 125, wild_cap: 80 },
]);

/// Cells the floor-churn scatters across each settle.
const SCATTER: u32 = 8;
/// Climate hotspots per element.
pub const HOTSPOTS: u32 = 2;

#[derive(Clone, Debug)]
pub struct Terrain {
    /// Cells per side.
    pub side: usize,
    /// One saturation plane per element, row-major, `side × side`.
    pub sat: [Vec<u16>; Element::COUNT],
    /// Deposit demand accumulated since the last settle, milli-units per cell.
    dep: [Vec<u64>; Element::COUNT],
    /// Consume demand, same layout. Recorded at the *consumer's* cell; applied
    /// against the plane of the element that consumer eats.
    con: [Vec<u64>; Element::COUNT],
}

impl Terrain {
    pub fn new(side: usize) -> Terrain {
        let side = side.max(1);
        let n = side * side;
        Terrain {
            side,
            sat: std::array::from_fn(|_| vec![0u16; n]),
            dep: std::array::from_fn(|_| vec![0u64; n]),
            con: std::array::from_fn(|_| vec![0u64; n]),
        }
    }

    #[inline]
    pub fn cells(&self) -> usize {
        self.side * self.side
    }

    /// The cell a position falls in. Positions live in `[0, side]`; the far
    /// edge folds into the last cell.
    #[inline]
    pub fn cell_of(&self, p: V2) -> usize {
        let s = self.side as i32;
        let x = p.x.floor_int().clamp(0, s - 1) as usize;
        let y = p.y.floor_int().clamp(0, s - 1) as usize;
        y * self.side + x
    }

    /// Centre of a cell, for spawning out of it.
    pub fn cell_center(&self, cell: usize) -> V2 {
        use crate::fx::Fx;
        let x = (cell % self.side) as i32;
        let y = (cell / self.side) as i32;
        V2::new(
            Fx::from_int(x) + Fx::HALF,
            Fx::from_int(y) + Fx::HALF,
        )
    }

    #[inline]
    pub fn sat_at(&self, e: Element, cell: usize) -> u16 {
        self.sat[e.index()][cell]
    }

    /// Dominant element and its saturation for a cell — the renderer's view.
    pub fn dominant(&self, cell: usize) -> (Element, u16) {
        let mut best = Element::Wood;
        let mut val = 0u16;
        for e in Element::ALL {
            let s = self.sat[e.index()][cell];
            if s > val {
                val = s;
                best = e;
            }
        }
        (best, val)
    }

    // ------------------------------------------------------------------
    // Demand intake (between settles).
    // ------------------------------------------------------------------

    #[inline]
    pub fn add_dep(&mut self, e: Element, cell: usize, milli: u64) {
        let d = &mut self.dep[e.index()][cell];
        *d = d.saturating_add(milli);
    }

    #[inline]
    pub fn add_con(&mut self, e: Element, cell: usize, milli: u64) {
        let c = &mut self.con[e.index()][cell];
        *c = c.saturating_add(milli);
    }

    pub fn dep_total(&self, e: Element) -> u64 {
        self.dep[e.index()].iter().fold(0u64, |a, v| a.saturating_add(*v))
    }

    pub fn con_total(&self, e: Element) -> u64 {
        self.con[e.index()].iter().fold(0u64, |a, v| a.saturating_add(*v))
    }

    // ------------------------------------------------------------------
    // Settle: turn granted demand into field change.
    // ------------------------------------------------------------------

    /// Apply a deposit grant for element `e`: `paid` units distributed across
    /// cells in proportion to their share of `total` demand, plus `forced`
    /// units (the floor nobody asked for) scattered at hash-chosen cells.
    pub fn apply_deposit(&mut self, e: Element, paid: u64, total: u64, forced: u64, seed: u64, tick: u64) {
        if paid > 0 && total > 0 {
            let plane = &mut self.sat[e.index()];
            for (i, d) in self.dep[e.index()].iter().enumerate() {
                if *d == 0 {
                    continue;
                }
                let add = ((*d as u128 * paid as u128) / total as u128) as u64;
                plane[i] = plane[i].saturating_add(add.min(u16::MAX as u64) as u16);
            }
        }
        if forced > 0 {
            self.scatter(e, forced, seed, tick, false);
        }
    }

    /// Apply a consume grant for element `e`. Consumption recorded at the
    /// consumer's cell is taken from the plane of the element it *eats* —
    /// Fire's grazing thins the Wood beneath it.
    pub fn apply_consume(&mut self, e: Element, paid: u64, total: u64, forced: u64, seed: u64, tick: u64) {
        let prey = e.eats();
        if paid > 0 && total > 0 {
            let plane = &mut self.sat[prey.index()];
            for (i, d) in self.con[e.index()].iter().enumerate() {
                if *d == 0 {
                    continue;
                }
                let take = ((*d as u128 * paid as u128) / total as u128) as u64;
                plane[i] = plane[i].saturating_sub(take.min(u16::MAX as u64) as u16);
            }
        }
        if forced > 0 {
            self.scatter(prey, forced, seed, tick, true);
        }
    }

    /// Floor churn: deposition (or consumption) nobody asked for, landed at
    /// hash-chosen cells. This is the world acting on its own — and it is
    /// deliberately *visible*, because a place nobody plays should still move.
    fn scatter(&mut self, e: Element, amount: u64, seed: u64, tick: u64, take: bool) {
        let n = self.cells() as u32;
        let each = (amount / SCATTER as u64).max(1).min(u16::MAX as u64) as u16;
        let plane = &mut self.sat[e.index()];
        for k in 0..SCATTER {
            let salt = (e.index() as u32) << 8 | k;
            let cell = rand_below(seed, tick, salt, Channel::WildChurn, n) as usize;
            plane[cell] = if take {
                plane[cell].saturating_sub(each)
            } else {
                plane[cell].saturating_add(each)
            };
        }
    }

    /// Clear the demand grids for the next accumulation period.
    pub fn clear_demand(&mut self) {
        for p in self.dep.iter_mut().chain(self.con.iter_mut()) {
            p.fill(0);
        }
    }

    // ------------------------------------------------------------------
    // The field's own churn.
    // ------------------------------------------------------------------

    /// Operators 3–7, in specification order. `tick` is the sim tick of the
    /// settle this runs inside.
    pub fn ops(&mut self, attrs: &PerElement<TerrainAttrs>, seed: u64, tick: u64) {
        self.op_overcome(attrs);
        self.op_generate(attrs);
        self.op_influx(attrs, seed);
        self.op_decay(attrs);
        self.op_diffuse(attrs);
        let _ = tick; // reserved for operators that will need per-tick draws
    }

    /// Star erosion: `sat[E]` erodes `sat[O(E)]` in the same cell.
    fn op_overcome(&mut self, attrs: &PerElement<TerrainAttrs>) {
        for e in Element::ALL {
            let rate = attrs[e].overcome.min(1000) as u32;
            if rate == 0 {
                continue;
            }
            let victim = e.suppresses().index();
            for i in 0..self.cells() {
                let cut = (self.sat[e.index()][i] as u32 * rate / 1000) as u16;
                self.sat[victim][i] = self.sat[victim][i].saturating_sub(cut);
            }
        }
    }

    /// Ring maturation: a share of `sat[E]` becomes `sat[G(E)]`. This is the
    /// whole succession mechanic.
    fn op_generate(&mut self, attrs: &PerElement<TerrainAttrs>) {
        for e in Element::ALL {
            let rate = attrs[e].generate.min(1000) as u32;
            if rate == 0 {
                continue;
            }
            let next = e.generates().index();
            for i in 0..self.cells() {
                let moved = (self.sat[e.index()][i] as u32 * rate / 1000) as u16;
                self.sat[e.index()][i] -= moved;
                self.sat[next][i] = self.sat[next][i].saturating_add(moved);
            }
        }
    }

    /// Climate: each element seeps in around its hotspots. Hotspot geography
    /// is a pure function of the world seed — no state, no hash impact.
    fn op_influx(&mut self, attrs: &PerElement<TerrainAttrs>, seed: u64) {
        let side = self.side as i32;
        let radius = (side / 10).max(2);
        for e in Element::ALL {
            let amount = attrs[e].influx;
            if amount == 0 {
                continue;
            }
            for k in 0..HOTSPOTS {
                let (cx, cy) = hotspot(seed, e, k, self.side);
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let x = cx as i32 + dx;
                        let y = cy as i32 + dy;
                        if x < 0 || y < 0 || x >= side || y >= side {
                            continue;
                        }
                        // Manhattan falloff: cheap, and the diamond it makes
                        // reads as geography rather than as a stamp.
                        let d = dx.abs() + dy.abs();
                        if d > radius {
                            continue;
                        }
                        let add = (amount * (radius - d) as u32 / radius as u32) as u16;
                        let cell = (y * side + x) as usize;
                        let p = &mut self.sat[e.index()][cell];
                        *p = p.saturating_add(add);
                    }
                }
            }
        }
    }

    /// Passive forgetting.
    fn op_decay(&mut self, attrs: &PerElement<TerrainAttrs>) {
        for e in Element::ALL {
            let rate = attrs[e].decay.min(1000) as u32;
            if rate == 0 {
                continue;
            }
            for v in self.sat[e.index()].iter_mut() {
                *v -= (*v as u32 * rate / 1000) as u16;
            }
        }
    }

    /// Radius-1 diffusion. The moved share splits between the neighbours that
    /// exist, so edges neither leak nor pile up. An accumulation buffer keeps
    /// the result independent of cell visit order.
    fn op_diffuse(&mut self, attrs: &PerElement<TerrainAttrs>) {
        let side = self.side;
        let n = self.cells();
        let mut acc = vec![0i32; n];
        for e in Element::ALL {
            let rate = attrs[e].diffuse.min(1000) as u32;
            if rate == 0 {
                continue;
            }
            acc.fill(0);
            let plane = &self.sat[e.index()];
            for y in 0..side {
                for x in 0..side {
                    let i = y * side + x;
                    let out = (plane[i] as u32 * rate / 1000) as i32;
                    if out == 0 {
                        continue;
                    }
                    let mut nbs = [0usize; 4];
                    let mut cnt = 0;
                    if x > 0 { nbs[cnt] = i - 1; cnt += 1; }
                    if x + 1 < side { nbs[cnt] = i + 1; cnt += 1; }
                    if y > 0 { nbs[cnt] = i - side; cnt += 1; }
                    if y + 1 < side { nbs[cnt] = i + side; cnt += 1; }
                    let share = out / cnt as i32;
                    let moved = share * cnt as i32; // remainder stays home
                    acc[i] -= moved;
                    for nb in &nbs[..cnt] {
                        acc[*nb] += share;
                    }
                }
            }
            let plane = &mut self.sat[e.index()];
            for i in 0..n {
                let v = plane[i] as i32 + acc[i];
                plane[i] = v.clamp(0, u16::MAX as i32) as u16;
            }
        }
    }

    // ------------------------------------------------------------------
    // Incarnation-event support.
    // ------------------------------------------------------------------

    /// A terrain-gated event site for `e`: one cell at or above the threshold,
    /// chosen uniformly among all eligible cells by stateless draw.
    pub fn gated_site(&self, e: Element, threshold: u16, seed: u64, tick: u64) -> Option<usize> {
        if threshold == 0 {
            return None;
        }
        let plane = &self.sat[e.index()];
        let eligible = plane.iter().filter(|s| **s >= threshold).count();
        if eligible == 0 {
            return None;
        }
        let pick = rand_below(seed, tick, e.index() as u32, Channel::Events, eligible as u32) as usize;
        plane
            .iter()
            .enumerate()
            .filter(|(_, s)| **s >= threshold)
            .nth(pick)
            .map(|(i, _)| i)
    }

    /// A wild event site — the lightning strike. Pure chance, anywhere,
    /// regardless of terrain. This is what guarantees every race is always
    /// joinable on every server.
    pub fn wild_site(&self, e: Element, permille: u16, seed: u64, tick: u64) -> Option<usize> {
        let salt = 0x57 << 8 | e.index() as u32;
        if !rand_chance(seed, tick, salt, Channel::Events, permille.min(1000) as u32, 1000) {
            return None;
        }
        Some(rand_below(seed, tick, salt ^ 0xF00, Channel::Events, self.cells() as u32) as usize)
    }
}

/// Where element `e`'s `k`-th climate hotspot sits. A pure function of the
/// seed: every process that knows the seed knows the climate.
pub fn hotspot(seed: u64, e: Element, k: u32, side: usize) -> (usize, usize) {
    let salt = (e.index() as u32) * 16 + k;
    let x = rand_below(seed, 0xC1_1A7E, salt * 2, Channel::Climate, side as u32) as usize;
    let y = rand_below(seed, 0xC1_1A7E, salt * 2 + 1, Channel::Climate, side as u32) as usize;
    (x, y)
}

impl Hashable for Terrain {
    fn hash_into(&self, h: &mut Hasher) {
        h.u64(self.side as u64);
        for p in &self.sat {
            h.u16_lanes(p);
        }
        // Demand grids are sparse nearly always; hash only the live entries,
        // with their indices so position matters.
        for planes in [&self.dep, &self.con] {
            for p in planes.iter() {
                for (i, v) in p.iter().enumerate() {
                    if *v != 0 {
                        h.u32(i as u32).u64(*v);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::Fx;

    fn attrs() -> PerElement<TerrainAttrs> {
        TERRAIN
    }

    #[test]
    fn defaults_are_valid() {
        for e in Element::ALL {
            assert!(TERRAIN[e].is_valid(), "{}", e.name());
        }
    }

    #[test]
    fn cell_of_maps_and_clamps() {
        let t = Terrain::new(16);
        assert_eq!(t.cell_of(V2::ZERO), 0);
        assert_eq!(t.cell_of(V2::new(Fx::from_int(15), Fx::from_int(15))), 255);
        // The far edge folds inward instead of indexing out of bounds.
        assert_eq!(t.cell_of(V2::new(Fx::from_int(16), Fx::from_int(16))), 255);
        assert_eq!(t.cell_of(V2::new(-Fx::ONE, -Fx::ONE)), 0);
    }

    #[test]
    fn succession_flows_around_the_ring() {
        // Seed pure Fire in one spot with no decay and no diffusion: the
        // generating operator alone must walk it Fire → Earth → Metal → Water
        // → Wood. This is the claim that succession is the ring, verified.
        let mut t = Terrain::new(8);
        let mut a = PerElement::filled(TerrainAttrs {
            influx: 0, decay: 0, generate: 100, overcome: 0, diffuse: 0,
            ev_threshold: 0, ev_window: 100, wild: 0, wild_cap: 0,
        });
        let _ = &mut a;
        t.sat[Element::Fire.index()][10] = 60_000;

        let mut seen = PerElement::filled(false);
        for tick in 0..200u64 {
            t.ops(&a, 1, tick);
            for e in Element::ALL {
                if t.sat[e.index()][10] > 1000 {
                    *seen.get_mut(e) = true;
                }
            }
        }
        for e in Element::ALL {
            assert!(seen[e], "{} never received succession", e.name());
        }
    }

    #[test]
    fn overcoming_erodes_the_victim_in_place() {
        let mut t = Terrain::new(4);
        let a = attrs();
        // Water suppresses Fire.
        t.sat[Element::Water.index()][5] = 40_000;
        t.sat[Element::Fire.index()][5] = 40_000;
        let before = t.sat[Element::Fire.index()][5];
        t.op_overcome(&a);
        assert!(t.sat[Element::Fire.index()][5] < before);
    }

    #[test]
    fn influx_builds_biomes_from_nothing() {
        let mut t = Terrain::new(32);
        let a = attrs();
        for tick in 0..40 {
            t.ops(&a, 0xBEEF, tick);
        }
        for e in Element::ALL {
            let total: u64 = t.sat[e.index()].iter().map(|v| *v as u64).sum();
            assert!(total > 0, "{} grew nothing from its hotspots", e.name());
        }
    }

    #[test]
    fn no_absorbing_state_from_any_monoculture() {
        // Saturate the whole map with one element and let the field run. If
        // any single-element state is a fixed point, a race can be permanently
        // exterminated — the design's explicit nightmare. Every other element
        // must reappear.
        for mono in Element::ALL {
            let mut t = Terrain::new(16);
            let a = attrs();
            t.sat[mono.index()].fill(60_000);
            for tick in 0..600 {
                t.ops(&a, 42, tick);
            }
            for e in Element::ALL {
                let total: u64 = t.sat[e.index()].iter().map(|v| *v as u64).sum();
                assert!(
                    total > 0,
                    "world saturated with {} left {} at zero",
                    mono.name(),
                    e.name()
                );
            }
        }
    }

    #[test]
    fn diffusion_spreads_without_creating() {
        let mut t = Terrain::new(9);
        let a = PerElement::filled(TerrainAttrs {
            influx: 0, decay: 0, generate: 0, overcome: 0, diffuse: 300,
            ev_threshold: 0, ev_window: 100, wild: 0, wild_cap: 0,
        });
        let center = 4 * 9 + 4;
        t.sat[0][center] = 40_000;
        let before: u64 = t.sat[0].iter().map(|v| *v as u64).sum();
        for _ in 0..30 {
            t.op_diffuse(&a);
        }
        let after: u64 = t.sat[0].iter().map(|v| *v as u64).sum();
        assert!(after <= before, "diffusion created saturation");
        assert!(after >= before * 95 / 100, "diffusion lost more than rounding");
        // And it actually moved: the centre no longer holds everything.
        assert!(t.sat[0][center] < 20_000);
        assert!(t.sat[0][center - 1] > 0);
    }

    #[test]
    fn deposits_land_where_the_demand_was() {
        let mut t = Terrain::new(8);
        t.add_dep(Element::Fire, 10, 900_000);
        t.add_dep(Element::Fire, 50, 100_000);
        let total = t.dep_total(Element::Fire);
        t.apply_deposit(Element::Fire, 1000, total, 0, 1, 1);
        let a = t.sat[Element::Fire.index()][10];
        let b = t.sat[Element::Fire.index()][50];
        assert!(a > b * 8, "proportionality lost: {a} vs {b}");
        assert_eq!(a as u64 + b as u64, 1000);
    }

    #[test]
    fn consumption_takes_from_the_eaten_element() {
        let mut t = Terrain::new(8);
        // Fire eats Wood: Fire's consumption at cell 12 thins Wood there.
        t.sat[Element::Wood.index()][12] = 5000;
        t.add_con(Element::Fire, 12, 1_000_000);
        let total = t.con_total(Element::Fire);
        t.apply_consume(Element::Fire, 800, total, 0, 1, 1);
        assert_eq!(t.sat[Element::Wood.index()][12], 4200);
    }

    #[test]
    fn forced_churn_lands_even_with_zero_demand() {
        let mut t = Terrain::new(16);
        t.apply_deposit(Element::Metal, 0, 0, 800, 7, 3);
        let total: u64 = t.sat[Element::Metal.index()].iter().map(|v| *v as u64).sum();
        assert!(total > 0, "the floor produced nothing");
    }

    #[test]
    fn gated_site_respects_the_threshold() {
        let mut t = Terrain::new(16);
        assert_eq!(t.gated_site(Element::Wood, 9000, 1, 1), None);
        t.sat[Element::Wood.index()][77] = 9500;
        assert_eq!(t.gated_site(Element::Wood, 9000, 1, 1), Some(77));
    }

    #[test]
    fn wild_sites_eventually_fire_for_every_element() {
        let t = Terrain::new(16);
        for e in Element::ALL {
            let hit = (0..200u64).any(|tick| t.wild_site(e, 125, 99, tick).is_some());
            assert!(hit, "{} never got a wild event in 200 terrain ticks", e.name());
        }
    }

    #[test]
    fn ops_are_deterministic() {
        let mut a = Terrain::new(24);
        a.sat[1][100] = 30_000;
        a.sat[3][200] = 20_000;
        let mut b = a.clone();
        let at = attrs();
        for tick in 0..50 {
            a.ops(&at, 5, tick);
            b.ops(&at, 5, tick);
        }
        let mut ha = Hasher::new();
        a.hash_into(&mut ha);
        let mut hb = Hasher::new();
        b.hash_into(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn hotspots_are_stable_per_seed_and_differ_across_seeds() {
        let a = hotspot(1, Element::Water, 0, 96);
        assert_eq!(a, hotspot(1, Element::Water, 0, 96));
        let others: Vec<_> = (2..12u64).map(|s| hotspot(s, Element::Water, 0, 96)).collect();
        assert!(others.iter().any(|o| *o != a), "climate ignores the seed");
    }
}
