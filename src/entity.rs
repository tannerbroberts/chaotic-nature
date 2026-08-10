//! The simulated body.
//!
//! Stage 0 keeps this deliberately thin — enough state to exercise movement,
//! collision, ageing and every deposition channel, and nothing more. There is
//! no combat, no feeding and no terrain here yet; those arrive at S2 and S5.

use crate::element::Element;
use crate::fx::{Fx, V2};
use crate::hash::{Hashable, Hasher};
use crate::rand::{rand_range, rand_signed, Channel};
use crate::race::RaceAttrs;
#[cfg(test)]
use crate::race::attrs;

/// Displacement below which a tick does not count as an action for the
/// `OnAction` deposition channel. Standing still must not terraform, or Water
/// stops being action-dominant in practice.
pub const ACTION_THRESHOLD: Fx = Fx::ratio(1, 100);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entity {
    pub id: u32,
    pub element: Element,
    pub pos: V2,
    /// Unit vector. Movement is `heading * SPEED[element]`.
    pub heading: V2,
    pub age: u64,
    /// Rolled at birth from the race's base lifespan plus its variance, so a
    /// cohort born together does not die together.
    pub lifespan: u64,
    pub hp: i32,
    /// The metabolic account. Fed by biting the cell underfoot, drained by
    /// upkeep every tick; surplus becomes offspring, deficit becomes hp loss.
    pub energy: u32,
    /// Tick of the last explicit `SetHeading` — a steered body suppresses its
    /// own food-seeking for a while, so a player's hands beat instinct.
    pub steered_at: u64,
    pub alive: bool,
    /// Set by movement, read by demand accumulation, cleared each tick.
    pub acted: bool,
}

impl Entity {
    /// `a` is the spawning world's *live* row for this element, not the shipped
    /// table — the live view can retune between one birth and the next.
    pub fn spawn(id: u32, element: Element, pos: V2, seed: u64, tick: u64, a: &RaceAttrs) -> Entity {
        Entity {
            id,
            element,
            pos,
            heading: initial_heading(seed, tick, id),
            age: 0,
            lifespan: roll_lifespan(a, seed, tick, id),
            hp: 100,
            energy: a.birth_energy,
            steered_at: 0,
            alive: true,
            acted: false,
        }
    }

    #[inline]
    pub fn is_expired(&self) -> bool {
        self.age >= self.lifespan
    }
}

impl Hashable for Entity {
    fn hash_into(&self, h: &mut Hasher) {
        h.u32(self.id)
            .u8(self.element as u8)
            .i32(self.pos.x.raw())
            .i32(self.pos.y.raw())
            .i32(self.heading.x.raw())
            .i32(self.heading.y.raw())
            .u64(self.age)
            .u64(self.lifespan)
            .i32(self.hp)
            .u32(self.energy)
            .u64(self.steered_at)
            .bool(self.alive)
            .bool(self.acted);
    }
}

/// Deterministic per-individual lifespan. Variance is per-mille around the
/// race's base value.
pub fn roll_lifespan(a: &RaceAttrs, seed: u64, tick: u64, id: u32) -> u64 {
    let v = a.lifespan_variance as i32;
    if v == 0 {
        return a.lifespan.max(1);
    }
    let delta = rand_range(seed, tick, id, Channel::LifespanVariance, -v, v + 1);
    let scaled = (a.lifespan as i128) * (1000 + delta as i128) / 1000;
    scaled.max(1) as u64
}

/// A deterministic unit heading from the entity's birth coordinates.
pub fn initial_heading(seed: u64, tick: u64, id: u32) -> V2 {
    let x = rand_signed(seed, tick, id, Channel::Wander);
    let y = rand_signed(seed, tick, id.wrapping_add(0x5EED), Channel::Wander);
    let v = V2::new(x, y);
    if v.len_sq().is_zero() {
        V2::new(Fx::ONE, Fx::ZERO)
    } else {
        v.normalized()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_is_reproducible() {
        let f = attrs(Element::Fire);
        let a = Entity::spawn(7, Element::Fire, V2::ZERO, 99, 12, f);
        let b = Entity::spawn(7, Element::Fire, V2::ZERO, 99, 12, f);
        assert_eq!(a, b);
    }

    #[test]
    fn different_ids_get_different_headings() {
        let w = attrs(Element::Water);
        let a = Entity::spawn(1, Element::Water, V2::ZERO, 5, 0, w);
        let b = Entity::spawn(2, Element::Water, V2::ZERO, 5, 0, w);
        assert_ne!(a.heading, b.heading);
    }

    #[test]
    fn a_retuned_lifespan_reaches_the_bodies_born_after_it() {
        // The live view's whole premise: turn a knob, and the next thing born
        // is built to the new number.
        let mut a = *attrs(Element::Fire);
        a.lifespan_variance = 0;
        a.lifespan = 4242;
        let e = Entity::spawn(1, Element::Fire, V2::ZERO, 5, 0, &a);
        assert_eq!(e.lifespan, 4242);
    }

    #[test]
    fn initial_heading_is_unit_length() {
        for id in 0..500u32 {
            let h = initial_heading(3, 0, id);
            let l = h.len();
            assert!(
                (l - Fx::ONE).abs().raw() <= 512,
                "id {} heading length {:?}",
                id,
                l
            );
        }
    }

    #[test]
    fn lifespan_stays_inside_the_variance_band() {
        for e in Element::ALL {
            let a = attrs(e);
            let v = a.lifespan_variance as u128;
            let lo = (a.lifespan as u128) * (1000 - v) / 1000;
            let hi = (a.lifespan as u128) * (1000 + v) / 1000;
            for id in 0..400u32 {
                let l = roll_lifespan(a, 11, 0, id) as u128;
                assert!(l >= lo && l <= hi, "{} id {} → {}", e.name(), id, l);
            }
        }
    }

    #[test]
    fn a_cohort_does_not_die_together() {
        let a = attrs(Element::Fire);
        let spans: std::collections::BTreeSet<u64> =
            (0..200u32).map(|id| roll_lifespan(a, 1, 0, id)).collect();
        assert!(spans.len() > 100, "only {} distinct lifespans", spans.len());
    }

    #[test]
    fn speed_ordering_matches_the_fantasy() {
        let order = [
            Element::Fire,
            Element::Water,
            Element::Metal,
            Element::Wood,
            Element::Earth,
        ];
        for w in order.windows(2) {
            assert!(
                attrs(w[0]).speed > attrs(w[1]).speed,
                "{} should be faster than {}",
                w[0].name(),
                w[1].name()
            );
        }
    }

    #[test]
    fn everything_has_a_positive_speed_and_radius() {
        for e in Element::ALL {
            assert!(attrs(e).speed > Fx::ZERO, "{} is immobile", e.name());
            assert!(attrs(e).radius > Fx::ZERO, "{} has no body", e.name());
        }
    }
}
