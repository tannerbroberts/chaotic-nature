//! The simulation.
//!
//! The tick order below is part of the specification, not an implementation
//! detail. Reordering any two phases changes results and therefore invalidates
//! every recorded replay — treat it the way you would treat a wire format.

use crate::element::{Element, PerElement};
use crate::entity::{Entity, ACTION_THRESHOLD};
use crate::fx::{Fx, V2};
use crate::governor::{Governor, Grant};
use crate::hash::{Hashable, Hasher};
use crate::input::{CmdKind, Command, InputLog};
use crate::race::{attrs, Channel as DepChannel, RaceAttrs, MILLI, RACES, TERRAIN_PERIOD};
use crate::rand::{rand_signed, Channel};

/// Per-tick positional noise, so entities do not travel on perfect rails.
pub const JITTER: Fx = Fx::ratio(1, 400);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stats {
    pub births: u64,
    pub deaths: u64,
    pub collisions: u64,
    pub actions: u64,
    /// Total demand refused by the deposit governors. A rising value means
    /// somebody is pushing on a rate limit.
    pub deposit_clipped: u64,
    /// Total emitted purely to honour a floor. A rising value means a race is
    /// absent or idle and the world is turning over without it.
    pub deposit_forced: u64,
}

#[derive(Clone, Debug)]
pub struct World {
    pub seed: u64,
    pub tick: u64,
    /// Always sorted by ascending `id` — Invariant IV. Every phase that
    /// touches this vector must preserve that ordering.
    pub entities: Vec<Entity>,
    pub next_id: u32,
    /// The simulated square is `[0, size] × [0, size]` in cells.
    pub size: Fx,

    /// The tuning table this world is running, seeded from [`RACES`] and
    /// changeable at runtime through [`World::retune`]. It lives here rather
    /// than in a global so that the live view can turn a knob without any
    /// other world — a soak, a verification replay — seeing it.
    ///
    /// It is covered by [`World::state_hash`], so a retuned world never
    /// compares equal to an untuned one.
    pub races: PerElement<RaceAttrs>,

    deposit_gov: PerElement<Governor>,
    consume_gov: PerElement<Governor>,
    /// Accumulated in milli-units between terrain ticks.
    deposit_demand: PerElement<u64>,
    consume_demand: PerElement<u64>,

    pub last_deposit: PerElement<Grant>,
    pub last_consume: PerElement<Grant>,
    pub stats: Stats,
}

impl World {
    pub fn new(seed: u64, size_cells: i32) -> World {
        World {
            seed,
            tick: 0,
            entities: Vec::new(),
            next_id: 1,
            size: Fx::from_int(size_cells),
            races: RACES,
            deposit_gov: PerElement(Element::ALL.map(|e| Governor::new(attrs(e).deposit))),
            consume_gov: PerElement(Element::ALL.map(|e| Governor::new(attrs(e).consume))),
            deposit_demand: PerElement::filled(0),
            consume_demand: PerElement::filled(0),
            last_deposit: PerElement::default(),
            last_consume: PerElement::default(),
            stats: Stats::default(),
        }
    }

    /// Swap the tuning table on a running world.
    ///
    /// Rate bands reach their governors immediately; banked burst budget
    /// carries over, clamped to whatever the new band allows. Lifespan is the
    /// one knob that does *not* reach back — every body already alive keeps the
    /// span it rolled at birth, so lowering it thins the population by
    /// attrition rather than by mass execution.
    pub fn retune(&mut self, races: PerElement<RaceAttrs>) {
        self.races = races;
        for e in Element::ALL {
            self.deposit_gov.get_mut(e).set_band(races[e].deposit);
            self.consume_gov.get_mut(e).set_band(races[e].consume);
        }
    }

    /// A deterministic starting population, spread evenly across the five
    /// races and placed by hash rather than by sequence.
    pub fn seed_population(&mut self, per_race: u32) {
        for e in Element::ALL {
            for k in 0..per_race {
                let salt = (e.index() as u32) * 7919 + k;
                let x = self.scatter(salt, 0);
                let y = self.scatter(salt, 1);
                self.spawn(e, V2::new(x, y));
            }
        }
    }

    fn scatter(&self, salt: u32, axis: u32) -> Fx {
        let r = crate::rand::rand_below(
            self.seed,
            u64::from(axis),
            salt,
            Channel::SpawnPlacement,
            (self.size.floor_int().max(1)) as u32,
        );
        Fx::from_int(r as i32)
    }

    pub fn spawn(&mut self, element: Element, at: V2) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let a = self.races[element];
        let e = Entity::spawn(id, element, self.clamp_to_bounds(at), self.seed, self.tick, &a);
        // Ids are handed out ascending, so pushing preserves the sort.
        self.entities.push(e);
        self.stats.births += 1;
        *self.deposit_demand.get_mut(element) += a.deposit_per(DepChannel::OnBirth);
        *self.consume_demand.get_mut(element) += a.consume_per(DepChannel::OnBirth);
        id
    }

    fn clamp_to_bounds(&self, p: V2) -> V2 {
        V2::new(p.x.clamp(Fx::ZERO, self.size), p.y.clamp(Fx::ZERO, self.size))
    }

    fn find(&mut self, id: u32) -> Option<usize> {
        self.entities.binary_search_by_key(&id, |e| e.id).ok()
    }

    // ------------------------------------------------------------------
    // The tick.
    // ------------------------------------------------------------------

    pub fn step(&mut self, log: &InputLog) {
        self.phase_commands(log);
        self.phase_aging();
        self.phase_movement();
        self.phase_collisions();
        self.phase_settle();
        self.phase_reap();
        self.tick += 1;
    }

    /// 1 — apply every command stamped for this tick, in canonical order.
    fn phase_commands(&mut self, log: &InputLog) {
        for c in log.at(self.tick) {
            self.apply(*c);
        }
    }

    fn apply(&mut self, c: Command) {
        match c.kind {
            CmdKind::Spawn { element, at } => {
                self.spawn(element, at);
            }
            CmdKind::SetHeading { dir } => {
                if let Some(i) = self.find(c.entity) {
                    let n = dir.normalized();
                    if !n.len_sq().is_zero() {
                        self.entities[i].heading = n;
                    }
                }
            }
            CmdKind::Kill => {
                if let Some(i) = self.find(c.entity) {
                    self.entities[i].hp = 0;
                }
            }
        }
    }

    /// 2 — age every body and mark the expired ones. Death demand is charged
    /// here so a body that dies this tick still contributes its corpse.
    fn phase_aging(&mut self) {
        for i in 0..self.entities.len() {
            let e = &mut self.entities[i];
            if !e.alive {
                continue;
            }
            e.age += 1;
            e.acted = false;
            if e.is_expired() || e.hp <= 0 {
                e.alive = false;
                let el = e.element;
                let a = self.races[el];
                self.stats.deaths += 1;
                *self.deposit_demand.get_mut(el) += a.deposit_per(DepChannel::OnDeath);
                *self.consume_demand.get_mut(el) += a.consume_per(DepChannel::OnDeath);
            }
        }
    }

    /// 3 — move, jitter, and reflect off the bounds.
    fn phase_movement(&mut self) {
        let (seed, tick, size) = (self.seed, self.tick, self.size);
        let races = self.races;
        let mut acted: PerElement<u64> = PerElement::filled(0);

        for e in self.entities.iter_mut() {
            if !e.alive {
                continue;
            }
            let step = e.heading.scale(races[e.element].speed);
            let jitter = V2::new(
                rand_signed(seed, tick, e.id, Channel::MoveJitter) * JITTER,
                rand_signed(seed, tick, e.id.wrapping_add(0x9E37), Channel::MoveJitter) * JITTER,
            );
            let delta = step + jitter;
            let mut p = e.pos + delta;

            // Reflect rather than clamp, so a body never sticks to an edge.
            if p.x < Fx::ZERO {
                p.x = -p.x;
                e.heading.x = -e.heading.x;
            } else if p.x > size {
                p.x = size + size - p.x;
                e.heading.x = -e.heading.x;
            }
            if p.y < Fx::ZERO {
                p.y = -p.y;
                e.heading.y = -e.heading.y;
            } else if p.y > size {
                p.y = size + size - p.y;
                e.heading.y = -e.heading.y;
            }
            e.pos = V2::new(p.x.clamp(Fx::ZERO, size), p.y.clamp(Fx::ZERO, size));

            if delta.len_sq() > ACTION_THRESHOLD * ACTION_THRESHOLD {
                e.acted = true;
                *acted.get_mut(e.element) += 1;
            }
        }

        for (el, n) in acted.iter() {
            if *n > 0 {
                self.stats.actions += *n;
                *self.deposit_demand.get_mut(el) += races[el].deposit_per(DepChannel::OnAction) * *n;
                *self.consume_demand.get_mut(el) += races[el].consume_per(DepChannel::OnAction) * *n;
            }
        }
    }

    /// 4 — pairwise separation. O(n²) is correct and fast enough for Stage 0;
    /// a uniform-grid broadphase arrives with the terrain field at S1, and it
    /// must iterate cells in index order to stay deterministic.
    fn phase_collisions(&mut self) {
        let n = self.entities.len();
        let mut fix = vec![V2::ZERO; n];

        for i in 0..n {
            if !self.entities[i].alive {
                continue;
            }
            for j in (i + 1)..n {
                if !self.entities[j].alive {
                    continue;
                }
                let a = &self.entities[i];
                let b = &self.entities[j];
                let d = b.pos - a.pos;
                let min = self.races[a.element].radius + self.races[b.element].radius;
                let dist_sq = d.len_sq();
                if dist_sq >= min * min || dist_sq.is_zero() {
                    continue;
                }
                let dist = d.len();
                let overlap = (min - dist) * Fx::HALF;
                let push = d.normalized().scale(overlap);
                fix[i] = fix[i] - push;
                fix[j] = fix[j] + push;
                self.stats.collisions += 1;
            }
        }

        let size = self.size;
        for (i, e) in self.entities.iter_mut().enumerate() {
            if !e.alive || fix[i] == V2::ZERO {
                continue;
            }
            let p = e.pos + fix[i];
            e.pos = V2::new(p.x.clamp(Fx::ZERO, size), p.y.clamp(Fx::ZERO, size));
        }
    }

    /// 5 — at a terrain-tick boundary, charge existence and settle every
    /// governor. This is the only place demand becomes terrain change.
    fn phase_settle(&mut self) {
        if !(self.tick + 1).is_multiple_of(TERRAIN_PERIOD) {
            return;
        }

        let mut alive: PerElement<u64> = PerElement::filled(0);
        for e in &self.entities {
            if e.alive {
                *alive.get_mut(e.element) += 1;
            }
        }
        let races = self.races;
        for (el, n) in alive.iter() {
            if *n > 0 {
                *self.deposit_demand.get_mut(el) +=
                    races[el].deposit_per(DepChannel::OnExistence) * *n;
                *self.consume_demand.get_mut(el) +=
                    races[el].consume_per(DepChannel::OnExistence) * *n;
            }
        }

        for el in Element::ALL {
            let d = self.deposit_demand[el] / MILLI;
            let grant = self.deposit_gov.get_mut(el).settle(d);
            self.stats.deposit_clipped += grant.clipped;
            self.stats.deposit_forced += grant.forced;
            self.last_deposit[el] = grant;
            self.deposit_demand[el] = 0;

            let c = self.consume_demand[el] / MILLI;
            self.last_consume[el] = self.consume_gov.get_mut(el).settle(c);
            self.consume_demand[el] = 0;
        }
    }

    /// 6 — remove the dead. `retain` is order-preserving, so the id sort holds.
    fn phase_reap(&mut self) {
        self.entities.retain(|e| e.alive);
    }

    // ------------------------------------------------------------------

    pub fn alive_count(&self) -> usize {
        self.entities.iter().filter(|e| e.alive).count()
    }

    pub fn population(&self) -> PerElement<u32> {
        let mut p = PerElement::filled(0);
        for e in &self.entities {
            if e.alive {
                *p.get_mut(e.element) += 1;
            }
        }
        p
    }

    /// The canonical state hash. Everything that can affect a future tick must
    /// be in here — anything left out is a divergence this instrument cannot see.
    pub fn state_hash(&self) -> u64 {
        let mut h = Hasher::new();
        h.u64(self.seed)
            .u64(self.tick)
            .u32(self.next_id)
            .i32(self.size.raw())
            .u32(self.entities.len() as u32);

        for e in &self.entities {
            e.hash_into(&mut h);
        }
        // The tuning table is state now, so a retuned world must not hash the
        // same as an untuned one — otherwise `retune` is a silent divergence.
        for (_, a) in self.races.iter() {
            a.hash_into(&mut h);
        }
        for (_, g) in self.deposit_gov.iter() {
            g.hash_into(&mut h);
        }
        for (_, g) in self.consume_gov.iter() {
            g.hash_into(&mut h);
        }
        for (_, d) in self.deposit_demand.iter() {
            h.u64(*d);
        }
        for (_, d) in self.consume_demand.iter() {
            h.u64(*d);
        }
        for (_, g) in self.last_deposit.iter() {
            g.hash_into(&mut h);
        }
        for (_, g) in self.last_consume.iter() {
            g.hash_into(&mut h);
        }
        h.u64(self.stats.births)
            .u64(self.stats.deaths)
            .u64(self.stats.collisions)
            .u64(self.stats.actions)
            .u64(self.stats.deposit_clipped)
            .u64(self.stats.deposit_forced);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        let mut w = World::new(0xC0FFEE, 64);
        w.seed_population(8);
        w
    }

    #[test]
    fn entities_stay_sorted_by_id() {
        let mut w = world();
        let log = InputLog::new();
        for _ in 0..2000 {
            w.step(&log);
            let ids: Vec<u32> = w.entities.iter().map(|e| e.id).collect();
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            assert_eq!(ids, sorted, "id ordering broken at tick {}", w.tick);
        }
    }

    #[test]
    fn everything_stays_inside_the_bounds() {
        let mut w = world();
        let log = InputLog::new();
        for _ in 0..3000 {
            w.step(&log);
            for e in &w.entities {
                assert!(e.pos.x >= Fx::ZERO && e.pos.x <= w.size, "{:?}", e.pos);
                assert!(e.pos.y >= Fx::ZERO && e.pos.y <= w.size, "{:?}", e.pos);
            }
        }
    }

    #[test]
    fn fire_turns_over_many_times_before_earth_dies_once() {
        // The tempo axis, observed rather than asserted from the table.
        let mut w = World::new(7, 48);
        w.seed_population(6);
        let log = InputLog::new();
        for _ in 0..5000 {
            w.step(&log);
        }
        let pop = w.population();
        assert_eq!(pop[Element::Earth], 6, "Earth should not have died at all");
        assert_eq!(pop[Element::Fire], 0, "Fire should have burned out entirely");
        assert!(w.stats.deaths >= 6, "expected turnover, saw {}", w.stats.deaths);
    }

    #[test]
    fn governors_always_grant_inside_their_band() {
        let mut w = world();
        let log = InputLog::new();
        for _ in 0..4000 {
            w.step(&log);
            for el in Element::ALL {
                let b = attrs(el).deposit;
                let g = w.last_deposit[el];
                if g.granted == 0 {
                    continue; // before the first settlement
                }
                assert!(
                    g.granted >= b.floor as u64 && g.granted <= b.ceiling as u64,
                    "{} granted {} outside [{}, {}]",
                    el.name(),
                    g.granted,
                    b.floor,
                    b.ceiling
                );
            }
        }
    }

    #[test]
    fn an_extinct_race_still_churns_its_terrain() {
        // Nothing but Earth exists. Every other race must still be granted its
        // floor, which is what stops a lost biome becoming an absorbing state.
        let mut w = World::new(3, 32);
        for k in 0..4 {
            w.spawn(Element::Earth, V2::new(Fx::from_int(k * 3), Fx::from_int(k * 3)));
        }
        let log = InputLog::new();
        for _ in 0..(TERRAIN_PERIOD * 3) {
            w.step(&log);
        }
        for el in [Element::Fire, Element::Water, Element::Wood, Element::Metal] {
            assert_eq!(
                w.last_deposit[el].granted,
                attrs(el).deposit.floor as u64,
                "{} should be churning at its floor",
                el.name()
            );
            assert!(w.last_deposit[el].forced > 0);
        }
    }

    #[test]
    fn commands_are_applied_at_their_stamped_tick() {
        let mut w = World::new(1, 32);
        let id = w.spawn(Element::Metal, V2::new(Fx::from_int(16), Fx::from_int(16)));
        let mut log = InputLog::new();
        log.push(Command {
            tick: 10,
            entity: id,
            kind: CmdKind::Kill,
        });
        log.finalize();
        for _ in 0..10 {
            w.step(&log);
        }
        assert_eq!(w.alive_count(), 1, "still alive just before the command tick");
        w.step(&log);
        assert_eq!(w.alive_count(), 0, "killed on the command tick");
    }

    #[test]
    fn state_hash_notices_every_field_it_covers() {
        let mut a = world();
        let b = a.clone();
        assert_eq!(a.state_hash(), b.state_hash());
        let log = InputLog::new();
        a.step(&log);
        assert_ne!(a.state_hash(), b.state_hash());
    }
}
