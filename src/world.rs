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
use crate::terrain::{Terrain, TerrainAttrs, TERRAIN};

/// Per-tick positional noise, so entities do not travel on perfect rails.
pub const JITTER: Fx = Fx::ratio(1, 400);

/// Most incarnation events one element may have open at once. Keeps the sky
/// legible: a soul chooses among a handful of moments, not a wall of them.
pub const EVENTS_CAP: usize = 4;

/// An open incarnation event: a place and a window in which a soul may take a
/// body of this element. Unclaimed events do not simply vanish — at expiry the
/// world may use them itself, as a wildlife birth.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub id: u32,
    pub element: Element,
    pub cell: u32,
    pub opened: u64,
    pub closes: u64,
}

impl Hashable for Event {
    fn hash_into(&self, h: &mut Hasher) {
        h.u32(self.id)
            .u8(self.element as u8)
            .u32(self.cell)
            .u64(self.opened)
            .u64(self.closes);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stats {
    pub births: u64,
    pub deaths: u64,
    pub collisions: u64,
    pub actions: u64,
    pub births_by: PerElement<u64>,
    pub deaths_by: PerElement<u64>,
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
    /// Same story for the terrain's tuning.
    pub terrain_attrs: PerElement<TerrainAttrs>,
    /// Global movement multiplier, permille, applied on top of every race's
    /// speed. Mobility decides whether biomes coexist or smear into one — the
    /// most consequential single number in the ecology, so it gets a global
    /// control of its own.
    pub speed_scale: u16,

    /// The S1 field: five saturation planes plus the demand accumulated
    /// against them since the last settle.
    pub terrain: Terrain,

    /// Open incarnation events, ascending by id.
    pub events: Vec<Event>,
    next_event_id: u32,
    /// The most recent successful `Incarnate` claim, `(event id, entity id)`.
    /// This is how a client finds the body it was just granted.
    pub last_claim: Option<(u32, u32)>,

    deposit_gov: PerElement<Governor>,
    consume_gov: PerElement<Governor>,

    pub last_deposit: PerElement<Grant>,
    pub last_consume: PerElement<Grant>,
    pub stats: Stats,
}

impl World {
    pub fn new(seed: u64, size_cells: i32) -> World {
        let side = size_cells.max(1) as usize;
        World {
            seed,
            tick: 0,
            entities: Vec::new(),
            next_id: 1,
            size: Fx::from_int(size_cells),
            races: RACES,
            terrain_attrs: TERRAIN,
            speed_scale: 1000,
            terrain: Terrain::new(side),
            events: Vec::new(),
            next_event_id: 1,
            last_claim: None,
            deposit_gov: PerElement(Element::ALL.map(|e| Governor::new(attrs(e).deposit))),
            consume_gov: PerElement(Element::ALL.map(|e| Governor::new(attrs(e).consume))),
            last_deposit: PerElement::default(),
            last_consume: PerElement::default(),
            stats: Stats::default(),
        }
    }

    /// Swap the race tuning table on a running world.
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

    /// Swap the terrain tuning table. Takes effect at the next terrain tick.
    pub fn retune_terrain(&mut self, t: PerElement<TerrainAttrs>) {
        self.terrain_attrs = t;
    }

    /// Set the global movement multiplier, in permille of table speed.
    pub fn set_speed_scale(&mut self, permille: u16) {
        self.speed_scale = permille.clamp(10, 4000);
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
        let cell = self.terrain.cell_of(e.pos);
        // Ids are handed out ascending, so pushing preserves the sort.
        self.entities.push(e);
        self.stats.births += 1;
        *self.stats.births_by.get_mut(element) += 1;
        self.terrain.add_dep(element, cell, a.deposit_per(DepChannel::OnBirth));
        self.terrain.add_con(element, cell, a.consume_per(DepChannel::OnBirth));
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
                        // A commanded body holds its course: instinct yields
                        // to hands for a while.
                        self.entities[i].steered_at = self.tick;
                    }
                }
            }
            CmdKind::Kill => {
                if let Some(i) = self.find(c.entity) {
                    self.entities[i].hp = 0;
                }
            }
            CmdKind::Incarnate { event } => {
                // A claim on an event that has already closed — or never
                // existed — is a deterministic no-op: the moment passed.
                if let Some(i) = self.events.iter().position(|ev| ev.id == event) {
                    let ev = self.events.remove(i);
                    let at = self.terrain.cell_center(ev.cell as usize);
                    let id = self.spawn(ev.element, at);
                    self.last_claim = Some((ev.id, id));
                }
            }
        }
    }

    /// 2 — age, metabolise, feed, breed, starve, die. Death demand is charged
    /// here, at the corpse's cell, so a body that dies this tick still
    /// terraforms with its own remains.
    ///
    /// Population is not managed anywhere: it is the emergent sum of what
    /// happens in this function. Enough food → surplus → offspring. Not
    /// enough → starvation. An explosion or a collapse is a knob problem,
    /// and both are visible failures rather than prevented ones.
    fn phase_aging(&mut self) {
        let mut births: Vec<(Element, V2)> = Vec::new();

        for i in 0..self.entities.len() {
            let e = &mut self.entities[i];
            if !e.alive {
                continue;
            }
            e.age += 1;
            e.acted = false;
            let el = e.element;
            let a = self.races[el];
            let cell = self.terrain.cell_of(e.pos);

            // The meal cadence, staggered by id so a cohort does not all eat
            // on the same tick. A body eats what its cell actually holds of
            // the element it consumes — no more.
            if (self.tick + e.id as u64).is_multiple_of(RaceAttrs::FEED_PERIOD) {
                let bite = self.terrain.bite(el.eats(), cell, a.bite);
                if bite > 0 {
                    e.energy = (e.energy + bite * a.digest as u32 / 1000)
                        .min(crate::race::ENERGY_CAP);
                    // Deposit-on-consume fires only on a real meal — Metal
                    // writes to the world at the moment of refining.
                    self.terrain.add_dep(el, cell, a.deposit_per(DepChannel::OnConsume));
                }
                // Breeding rides the same cadence: a fed body splits.
                if e.energy >= a.repro_threshold && a.repro_cost > 0 {
                    e.energy -= a.repro_cost;
                    let off = V2::new(
                        crate::rand::rand_signed(self.seed, self.tick, e.id, Channel::Repro)
                            * Fx::from_int(2),
                        crate::rand::rand_signed(
                            self.seed,
                            self.tick,
                            e.id ^ 0x8888,
                            Channel::Repro,
                        ) * Fx::from_int(2),
                    );
                    births.push((el, e.pos + off));
                }
            }

            // Upkeep: being alive costs energy; running dry costs hp.
            e.energy = e.energy.saturating_sub(a.upkeep);
            if e.energy == 0 {
                e.hp -= a.starve_dmg as i32;
            }

            if e.is_expired() || e.hp <= 0 {
                e.alive = false;
                self.stats.deaths += 1;
                *self.stats.deaths_by.get_mut(el) += 1;
                self.terrain.add_dep(el, cell, a.deposit_per(DepChannel::OnDeath));
                self.terrain.add_con(el, cell, a.consume_per(DepChannel::OnDeath));
            }
        }

        // Births apply after the pass, in parent order — ids stay ascending.
        for (el, at) in births {
            self.spawn(el, at);
        }
    }

    /// How often a body reconsiders where the food is — hunger sharpens the
    /// nose — and how long an explicit `SetHeading` suppresses that instinct.
    const SEEK_PERIOD: u64 = 40;
    const SEEK_PERIOD_HUNGRY: u64 = 10;
    const STEER_GRACE: u64 = 300;
    /// How far ahead a body smells, in cells.
    const SEEK_REACH: i32 = 6;

    /// 3 — move, jitter, and reflect off the bounds. Action demand lands at
    /// the cell the body moved *into*, which is what makes Water's wake a
    /// trail rather than a point.
    ///
    /// Movement carries the first default behaviour: **food-seeking**. On a
    /// staggered cadence a body sniffs the four cardinal directions for the
    /// element it eats and turns toward a clearly richer cell. This is what
    /// lets wildlife find and hold a biome instead of drifting off it and
    /// starving — and what makes overgrazing self-correcting, because an
    /// emptied biome no longer pulls.
    fn phase_movement(&mut self) {
        let (seed, tick, size) = (self.seed, self.tick, self.size);
        let races = self.races;
        let scale = self.speed_scale as i64;

        for i in 0..self.entities.len() {
            // Seek reads the field, so it runs before the mutable borrow.
            let (id, el, pos, steered_at, energy, alive) = {
                let e = &self.entities[i];
                (e.id, e.element, e.pos, e.steered_at, e.energy, e.alive)
            };
            if !alive {
                continue;
            }
            // A hungry body sniffs four times as often.
            let period = if energy < races[el].birth_energy / 2 {
                Self::SEEK_PERIOD_HUNGRY
            } else {
                Self::SEEK_PERIOD
            };
            let seek = if (tick + id as u64).is_multiple_of(period)
                && tick.saturating_sub(steered_at) >= Self::STEER_GRACE
            {
                let prey = el.eats();
                let here = self.terrain.sat_at(prey, self.terrain.cell_of(pos));
                // Must beat the current cell by a quarter, or stay the course.
                let mut best = here + here / 4;
                let mut turn = None;
                let reach = Fx::from_int(Self::SEEK_REACH);
                // Eight compass directions, fixed order.
                for (dx, dy) in [
                    (1i32, 0i32), (-1, 0), (0, 1), (0, -1),
                    (1, 1), (1, -1), (-1, 1), (-1, -1),
                ] {
                    let probe = V2::new(
                        (pos.x + reach * dx).clamp(Fx::ZERO, size),
                        (pos.y + reach * dy).clamp(Fx::ZERO, size),
                    );
                    let s = self.terrain.sat_at(prey, self.terrain.cell_of(probe));
                    if s > best {
                        best = s;
                        turn = Some(V2::new(Fx::from_int(dx), Fx::from_int(dy)).normalized());
                    }
                }
                turn
            } else {
                None
            };

            let e = &mut self.entities[i];
            if let Some(dir) = seek {
                e.heading = dir;
            }
            let sp = Fx::from_raw((races[e.element].speed.raw() as i64 * scale / 1000) as i32);
            let step = e.heading.scale(sp);
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
                let el = e.element;
                let cell = self.terrain.cell_of(self.entities[i].pos);
                self.stats.actions += 1;
                self.terrain.add_dep(el, cell, races[el].deposit_per(DepChannel::OnAction));
                self.terrain.add_con(el, cell, races[el].consume_per(DepChannel::OnAction));
            }
        }
    }

    /// 4 — pairwise separation through a uniform-grid broadphase.
    ///
    /// Resource-based population means the body count is emergent, and a
    /// runaway is a legitimate world state the simulation must survive — so
    /// quadratic collision had to go. The grid is deterministic by
    /// construction: buckets are filled in ascending entity order, every
    /// entity scans a fixed neighbourhood in a fixed order, and each pair is
    /// processed exactly once from its lower index.
    fn phase_collisions(&mut self) {
        let n = self.entities.len();
        if n < 2 {
            return;
        }

        // Bucket edge: the largest distance at which any pair can touch, so a
        // colliding pair is never more than one bucket apart on either axis.
        let max_r = Element::ALL
            .iter()
            .map(|e| self.races[*e].radius)
            .fold(Fx::ZERO, |a, r| a.max(r));
        let edge = ((max_r.raw() as i64 * 2) >> 16).max(1) as i32 + 1;
        let side = self.size.floor_int().max(1);
        let gw = (side / edge + 1) as usize;
        let cells = gw * gw;

        // Counting sort into a flat grid: stable, allocation-light, and the
        // per-bucket entity order is ascending because the scatter pass runs
        // in ascending order.
        let key = |p: V2| -> usize {
            let bx = (p.x.floor_int().clamp(0, side - 1) / edge) as usize;
            let by = (p.y.floor_int().clamp(0, side - 1) / edge) as usize;
            by * gw + bx
        };
        let mut count = vec![0u32; cells + 1];
        for e in &self.entities {
            count[key(e.pos) + 1] += 1;
        }
        for c in 1..=cells {
            count[c] += count[c - 1];
        }
        let mut slot = count.clone();
        let mut order = vec![0u32; n];
        for (i, e) in self.entities.iter().enumerate() {
            let k = key(e.pos);
            order[slot[k] as usize] = i as u32;
            slot[k] += 1;
        }

        let mut fix = vec![V2::ZERO; n];
        for i in 0..n {
            if !self.entities[i].alive {
                continue;
            }
            let p = self.entities[i].pos;
            let bx = (p.x.floor_int().clamp(0, side - 1) / edge) as isize;
            let by = (p.y.floor_int().clamp(0, side - 1) / edge) as isize;
            // Fixed neighbourhood order: row-major over the 3×3 block.
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let nx = bx + dx;
                    let ny = by + dy;
                    if nx < 0 || ny < 0 || nx as usize >= gw || ny as usize >= gw {
                        continue;
                    }
                    let k = ny as usize * gw + nx as usize;
                    for oi in count[k]..count[k + 1] {
                        let j = order[oi as usize] as usize;
                        if j <= i || !self.entities[j].alive {
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

    /// 5 — the terrain tick: charge existence, settle every governor into the
    /// field, run the field's own operators, then let the field open and close
    /// incarnation events. This is the only place demand becomes terrain.
    fn phase_settle(&mut self) {
        if !(self.tick + 1).is_multiple_of(TERRAIN_PERIOD) {
            return;
        }
        let races = self.races;

        // Existence: presence itself, charged at each body's cell.
        for i in 0..self.entities.len() {
            let e = &self.entities[i];
            if !e.alive {
                continue;
            }
            let el = e.element;
            let cell = self.terrain.cell_of(e.pos);
            self.terrain.add_dep(el, cell, races[el].deposit_per(DepChannel::OnExistence));
            self.terrain.add_con(el, cell, races[el].consume_per(DepChannel::OnExistence));
        }

        // Governors: demand in, bounded grants out, grants into the field.
        // The grant is in whole units; the per-cell distribution divides by
        // the demand total in ITS OWN scale (milli), or every deposit lands
        // a thousandfold — a lesson learned from an 80-million-saturation
        // map feeding a phantom herd.
        for el in Element::ALL {
            let dtotal = self.terrain.dep_total(el);
            let grant = self.deposit_gov.get_mut(el).settle(dtotal / MILLI);
            self.stats.deposit_clipped += grant.clipped;
            self.stats.deposit_forced += grant.forced;
            self.last_deposit[el] = grant;
            let paid = grant.granted - grant.forced;
            self.terrain.apply_deposit(el, paid, dtotal, grant.forced, self.seed, self.tick);

            let ctotal = self.terrain.con_total(el);
            let cgrant = self.consume_gov.get_mut(el).settle(ctotal / MILLI);
            self.last_consume[el] = cgrant;
            let cpaid = cgrant.granted - cgrant.forced;
            self.terrain.apply_consume(el, cpaid, ctotal, cgrant.forced, self.seed, self.tick);
        }

        // The field's own churn.
        let ta = self.terrain_attrs;
        self.terrain.ops(&ta, self.seed, self.tick);
        self.terrain.clear_demand();

        self.phase_events();
    }

    /// Incarnation events: expire the closed (letting the world use unclaimed
    /// ones as wildlife births), then open new ones — terrain-gated where the
    /// field allows it, wild sparks anywhere as the floor under extinction.
    fn phase_events(&mut self) {
        let pop = self.population();
        let ta = self.terrain_attrs;

        // Expiries, ascending by id. An unclaimed event is the world's to
        // spend: below the wildlife cap, the birth happens anyway.
        let expired: Vec<Event> = self
            .events
            .iter()
            .filter(|ev| ev.closes <= self.tick)
            .copied()
            .collect();
        self.events.retain(|ev| ev.closes > self.tick);
        for ev in expired {
            if pop[ev.element] < ta[ev.element].wild_cap {
                let at = self.terrain.cell_center(ev.cell as usize);
                self.spawn(ev.element, at);
            }
        }

        // Openings: at most one gated and one wild per element per terrain
        // tick, under the concurrency cap.
        for el in Element::ALL {
            let open = self.events.iter().filter(|ev| ev.element == el).count();
            if open >= EVENTS_CAP {
                continue;
            }
            let a = ta[el];
            let mut sites: Vec<usize> = Vec::with_capacity(2);
            if let Some(cell) = self.terrain.gated_site(el, a.ev_threshold, self.seed, self.tick) {
                sites.push(cell);
            }
            if let Some(cell) = self.terrain.wild_site(el, a.wild, self.seed, self.tick) {
                // A wild strike on the cell already chosen adds nothing.
                if sites.first() != Some(&cell) {
                    sites.push(cell);
                }
            }
            for cell in sites.into_iter().take(EVENTS_CAP - open) {
                let id = self.next_event_id;
                self.next_event_id += 1;
                self.events.push(Event {
                    id,
                    element: el,
                    cell: cell as u32,
                    opened: self.tick,
                    closes: self.tick + a.ev_window as u64,
                });
            }
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
        // The tuning tables are state, so a retuned world must not hash the
        // same as an untuned one — otherwise `retune` is a silent divergence.
        for (_, a) in self.races.iter() {
            a.hash_into(&mut h);
        }
        for (_, a) in self.terrain_attrs.iter() {
            a.hash_into(&mut h);
        }
        h.u16(self.speed_scale);
        self.terrain.hash_into(&mut h);

        h.u32(self.events.len() as u32);
        for ev in &self.events {
            ev.hash_into(&mut h);
        }
        h.u32(self.next_event_id);
        match self.last_claim {
            Some((ev, ent)) => h.bool(true).u32(ev).u32(ent),
            None => h.bool(false),
        };

        for (_, g) in self.deposit_gov.iter() {
            g.hash_into(&mut h);
        }
        for (_, g) in self.consume_gov.iter() {
            g.hash_into(&mut h);
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
        for (_, v) in self.stats.births_by.iter() {
            h.u64(*v);
        }
        for (_, v) in self.stats.deaths_by.iter() {
            h.u64(*v);
        }
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
        // The tempo axis, observed rather than asserted from the table. Wild
        // events may replenish Fire, so the claim is about deaths, not
        // survivors: every original Fire died, no Earth did. Metabolism is
        // switched off so old age is the only clock in the room.
        let mut w = World::new(7, 48);
        let mut races = RACES;
        for e in Element::ALL {
            races[e].upkeep = 0;
        }
        w.retune(races);
        w.seed_population(6);
        let log = InputLog::new();
        for _ in 0..5000 {
            w.step(&log);
        }
        assert!(
            w.stats.deaths_by[Element::Fire] >= 6,
            "all six original Fire should have burned out, saw {}",
            w.stats.deaths_by[Element::Fire]
        );
        assert_eq!(w.stats.deaths_by[Element::Earth], 0, "no Earth should have died");
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
    fn terraforming_reaches_the_field() {
        // Deaths and presence must become saturation the map can show.
        let mut w = world();
        let log = InputLog::new();
        for _ in 0..(TERRAIN_PERIOD * 20) {
            w.step(&log);
        }
        let total: u64 = Element::ALL
            .iter()
            .flat_map(|e| w.terrain.sat[e.index()].iter())
            .map(|v| *v as u64)
            .sum();
        assert!(total > 0, "twenty terrain ticks moved nothing into the field");
    }

    #[test]
    fn an_empty_world_self_populates() {
        // No seed population, no commands. Influx builds terrain, wild events
        // spark, unclaimed expiries become wildlife: life arises on its own.
        let mut w = World::new(3, 32);
        let log = InputLog::new();
        for _ in 0..(TERRAIN_PERIOD * 120) {
            w.step(&log);
        }
        assert!(
            w.stats.births > 0,
            "an empty server should have grown wildlife by now"
        );
    }

    #[test]
    fn events_open_and_have_windows() {
        let mut w = World::new(0xE0E0, 32);
        let log = InputLog::new();
        for _ in 0..(TERRAIN_PERIOD * 60) {
            w.step(&log);
            for ev in &w.events {
                assert!(ev.closes > ev.opened, "event with a non-window");
                assert!((ev.cell as usize) < w.terrain.cells());
            }
        }
        assert!(w.next_event_id > 1, "sixty terrain ticks opened no events at all");
    }

    #[test]
    fn every_element_gets_events_eventually() {
        // The wild backstop working: joinability does not depend on terrain.
        let mut w = World::new(0xAB, 32);
        let log = InputLog::new();
        let mut seen = PerElement::filled(false);
        for _ in 0..(TERRAIN_PERIOD * 200) {
            w.step(&log);
            for ev in &w.events {
                *seen.get_mut(ev.element) = true;
            }
            if Element::ALL.iter().all(|e| seen[*e]) {
                return;
            }
        }
        for e in Element::ALL {
            assert!(seen[e], "{} never opened an event", e.name());
        }
    }

    #[test]
    fn incarnate_claims_the_event_and_reports_the_body() {
        let mut w = World::new(0xCAFE, 32);
        let log = InputLog::new();
        // Run until an event exists.
        while w.events.is_empty() {
            w.step(&log);
            assert!(w.tick < 200_000, "no event ever opened");
        }
        let ev = w.events[0];
        let mut claim = InputLog::new();
        claim.push(Command {
            tick: w.tick,
            entity: 0,
            kind: CmdKind::Incarnate { event: ev.id },
        });
        claim.finalize();
        let before = w.alive_count();
        w.step(&claim);
        let (cev, cent) = w.last_claim.expect("claim should register");
        assert_eq!(cev, ev.id);
        assert_eq!(w.alive_count(), before + 1);
        let body = w.entities.iter().find(|e| e.id == cent).expect("body exists");
        assert_eq!(body.element, ev.element);
        assert_eq!(
            w.terrain.cell_of(body.pos) as u32,
            ev.cell,
            "born at the event's cell"
        );
        assert!(
            !w.events.iter().any(|e| e.id == ev.id),
            "claimed event should be gone"
        );
    }

    #[test]
    fn incarnate_on_a_dead_event_is_a_no_op() {
        let mut w = World::new(5, 32);
        let mut log = InputLog::new();
        log.push(Command {
            tick: 0,
            entity: 0,
            kind: CmdKind::Incarnate { event: 999_999 },
        });
        log.finalize();
        w.step(&log);
        assert_eq!(w.alive_count(), 0);
        assert_eq!(w.last_claim, None);
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

    #[test]
    fn state_hash_notices_the_terrain() {
        let a = world();
        let mut b = a.clone();
        b.terrain.sat[2][100] ^= 1;
        assert_ne!(a.state_hash(), b.state_hash(), "a flipped saturation bit was invisible");
    }

    #[test]
    fn a_body_starves_without_food() {
        // Barren world: no influx, no wild events, nothing to eat. One Metal
        // burns through its birth energy, then its hp, and dies long before
        // its 12-hour lifespan.
        let mut w = World::new(11, 32);
        let mut ta = crate::terrain::TERRAIN;
        let mut races = RACES;
        for e in Element::ALL {
            ta[e].wild = 0;
            // A body's own deposits can cross the event gate and host a
            // successor — the rebirth loop working. Shut it here: this test
            // is about metabolism in isolation, so the rain stops too.
            ta[e].ev_threshold = u16::MAX;
            races[e].deposit = crate::race::RateBand::new(0, 0, 0, 1);
        }
        w.retune_terrain(ta);
        w.retune(races);
        w.spawn(Element::Metal, V2::new(Fx::from_int(16), Fx::from_int(16)));
        let log = InputLog::new();
        for _ in 0..2000 {
            w.step(&log);
        }
        assert_eq!(w.stats.deaths_by[Element::Metal], 1, "starvation should have killed it");
        assert_eq!(w.alive_count(), 0);
    }

    #[test]
    fn a_fed_body_breeds_a_dynasty() {
        // The other half of resource-based population: stand a long-lived
        // Earth on rich Fire ground (Earth eats Fire) and offspring follow
        // from surplus alone — no restocking, no commands.
        let mut w = World::new(13, 32);
        let mut ta = crate::terrain::TERRAIN;
        let mut races = RACES;
        for e in Element::ALL {
            ta[e].wild = 0;
            ta[e].ev_threshold = u16::MAX;
            races[e].deposit = crate::race::RateBand::new(0, 0, 0, 1);
        }
        w.retune_terrain(ta);
        w.retune(races);
        for cell in 0..w.terrain.cells() {
            w.terrain.sat[Element::Fire.index()][cell] = 50_000;
        }
        w.spawn(Element::Earth, V2::new(Fx::from_int(16), Fx::from_int(16)));
        let log = InputLog::new();
        for _ in 0..4000 {
            w.step(&log);
        }
        assert!(
            w.stats.births_by[Element::Earth] >= 3,
            "a fed Earth should have bred, births: {}",
            w.stats.births_by[Element::Earth]
        );
        assert!(w.alive_count() >= 3);
    }

    #[test]
    fn population_is_resource_limited() {
        // Default knobs, no restocking, long run: population must neither
        // vanish nor grow without bound. Two probes — a ceiling, and a check
        // that growth has stopped accelerating by the end.
        let mut w = World::new(0xEC0, 48);
        w.seed_population(10);
        let log = InputLog::new();
        for _ in 0..20_000 {
            w.step(&log);
        }
        let mid = w.alive_count();
        for _ in 0..10_000 {
            w.step(&log);
        }
        let end = w.alive_count();
        assert!(end > 0, "the world starved out entirely");
        assert!(
            end < 2500,
            "population ran away on default knobs: {end} — the defaults need retuning"
        );
        assert!(
            end < mid + mid / 2 + 50,
            "population still accelerating on default knobs: {mid} → {end}"
        );
    }

    #[test]
    fn the_global_speed_scale_slows_everybody() {
        // Same seed, same body, empty terrain (so seek stays quiet): a world
        // at quarter scale must cover far less ground.
        let start = V2::new(Fx::from_int(48), Fx::from_int(48));
        let mut fast = World::new(21, 96);
        let mut slow = World::new(21, 96);
        slow.set_speed_scale(250);
        fast.spawn(Element::Water, start);
        slow.spawn(Element::Water, start);
        let log = InputLog::new();
        for _ in 0..100 {
            fast.step(&log);
            slow.step(&log);
        }
        let d = |w: &World| (w.entities[0].pos - start).len();
        assert!(
            d(&fast) > d(&slow) * 2,
            "quarter scale should travel far less: {:?} vs {:?}",
            d(&fast),
            d(&slow)
        );
    }
}