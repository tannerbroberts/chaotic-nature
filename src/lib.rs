//! Pentagram — Stage 0: the determinism skeleton.
//!
//! Exit condition for this stage: 10 000 ticks replay bit-identical from
//! `(seed, input_log)`, asserted at every tick rather than only at the end.
//!
//! # The invariants this crate exists to hold
//!
//! | | | Where it lives |
//! |---|---|---|
//! | I   | Bounded propagation — nothing acts instantly at a distance | *(S1: terrain diffusion cap)* |
//! | II  | No floating point in simulation state | [`fx`] |
//! | III | Randomness is stateless | [`rand`] |
//! | IV  | Iteration order is defined | [`world`], [`element::PerElement`] |
//! | V   | Inputs replicate; state does not | [`input`] |
//! | VI  | Every tick is reproducible | [`replay`] |
//! | VII | Bounded churn — rates are floored and capped | [`governor`] |
//!
//! Invariant I has no home yet because there is no field to diffuse. It
//! arrives with the terrain grid at Stage 1, and the band-width arithmetic
//! that depends on it arrives at Stage 3.
//!
//! # Reading order
//!
//! [`fx`] and [`rand`] are the foundation — everything else is downstream of
//! those two files being correct. [`element`] is the five-cycle. [`race`] and
//! [`governor`] carry the design's rate model. [`world`] is the tick loop, and
//! its phase order is a wire format, not an implementation detail.

pub mod element;
pub mod entity;
pub mod fx;
pub mod governor;
pub mod hash;
pub mod input;
pub mod race;
pub mod rand;
pub mod replay;
pub mod terrain;
pub mod tuning;
pub mod world;

pub use element::{Element, PerElement};
pub use fx::{Fx, V2};
pub use governor::{Governor, Grant};
pub use input::{CmdKind, Command, InputLog};
pub use race::{attrs, RaceAttrs, RateBand, TERRAIN_PERIOD};
pub use replay::{record, verify, Divergence, Trace};
pub use terrain::{Terrain, TerrainAttrs, TERRAIN};
pub use world::{Event, World};
