# Chaotic Nature

A five-element MMO built on a deterministic simulation core.

```
Cargo.toml     the project is the simulation
src/           the sim — authoritative for everything
  race.rs      ← the tuning table; the file you will edit most
client/        Godot 4 renderer (visual layer only)
legacy/        superseded pre-consolidation architecture, wired to nothing
docs/          architecture notes
```

**The sim is authoritative.** The Godot client renders and interpolates; it does
not own a tick clock, pathfinding, or any state. Where the two projects
disagreed about who decides what is true, the sim won and the loser is in
`legacy/` with a note explaining what replaced it.

## Stage 0 — the determinism skeleton

Everything else is downstream of this crate being correct, so it is small, has
zero dependencies, and is tested harder than its size suggests.

**Exit condition, met:** 10 000 ticks replay bit-identically from
`(seed, input_log)`, asserted at *every* tick rather than only at the end.

## The loop

```
chaos                  live view — change a number, watch what the world does
chaos watch            same, rebuilding whenever src/ changes
chaos edit             open the tuning table
chaos verify           the determinism exit condition
chaos soak [ticks]     long headless run + per-race report
chaos test             full suite (82 tests)
```

`chaos` lives in `~/.local/bin` and points at this directory; set `CHAOS_ROOT`
to move it. Flags pass through: `chaos --speed 600 --pop 40 --size 128`.

**Everything tunable is in `src/race.rs`.** One table, five rows. Lifespan,
speed, radius, deposit and consume units, rate bands, and channel mixes.

> Honest caveat for Stage 0: the grid in the live view shows diffusion, not
> ecology — there is no terrain to be attracted to and no feeding yet, so
> bodies mostly wander and bump. The readouts that *are* meaningful right now
> are the population sparklines and the governor states. The spatial view
> starts earning its place at S1 (terrain) and becomes the main instrument at
> S2 (plants and animals).

## The invariants

| | | Where it lives |
|---|---|---|
| I | Bounded propagation — nothing acts instantly at a distance | *S1: terrain diffusion cap* |
| II | No floating point in simulation state | `fx.rs` |
| III | Randomness is stateless | `rand.rs` |
| IV | Iteration order is defined | `world.rs`, `element::PerElement` |
| V | Inputs replicate; state does not | `input.rs` |
| VI | Every tick is reproducible | `replay.rs` |
| VII | Bounded churn — rates are floored and capped | `governor.rs` |

Invariant I has no home yet because there is no field to diffuse. It arrives
with the terrain grid at S1, and the band-width arithmetic that depends on it
arrives at S3.

### Three things that are easy to break later

**Overflow behaviour must match across profiles.** Rust panics on overflow in
debug and wraps in release — two different simulations from one source tree.
`Cargo.toml` sets `overflow-checks = true` in every profile. Do not remove it.

**The RNG has no state.** `rand(seed, tick, entity, channel)` is a pure hash.
Two territories simulating the same entity inside a shared band draw the same
number without exchanging a byte, which a sequential generator cannot do. The
`channel` argument keeps independent decisions independent, so adding a random
choice to foraging never shifts the stream feeding collision — and every replay
recorded before the change stays valid. Never renumber an existing channel.

**The tick phase order is a wire format.** `commands → aging → movement →
collisions → settle → reap`. Reordering any two phases changes results and
invalidates every recorded trace.

## The rate model

`deposit_unit` is what one body writes to the terrain over its **entire life**.
Each channel's per-mille share is spread across however many times that channel
actually fires in a life — birth and death once, existence once per terrain
tick, actions and meals at their own cadence. That makes the §3.1 parity rule
something the code computes rather than something the table hopes for:

```
race    lifespan            pressure   dominant channel
Fire        800 ticks  (8m)     2625   death        — terraforms by dying
Water      3500 ticks  (35m)    2542   action       — terraforms by flowing
Wood      15000 ticks  (2.5h)   2533   existence
Metal     72000 ticks  (12h)    2527   consume      — only at the forge
Earth   2016000 ticks  (14d)    2529   existence    — terraforms by staying
```

Lifespans span 2520×. Terraforming pressure spans 1.04×. That is the tempo axis
working: wildly different rhythms, near-identical total effect on the map.

### The governor

Every race's deposition and consumption passes through a rate band, per terrain
tick, per territory. It guarantees three things regardless of player behaviour:

- **Never below the floor** — emitted even at zero demand, even if the race is
  extinct here. An emptied server keeps turning over, which is what stops a lost
  biome from becoming an absorbing state.
- **Never above the ceiling** — no amount of coordination or exploitation moves
  more terrain in one tick than this.
- **Long-run average converges to nominal** — bursting spends a bucket that only
  refills at nominal, so maximum effort and minimum effort differ in *timing*,
  not in total.

Together these bound the terrain state at `T + k` before any player has decided
anything, which is what forecastable terrain actually requires.

`Grant` reports `granted`, `forced` (emitted only to honour the floor — the race
is idle or absent) and `clipped` (demand refused — somebody is pushing a rate
limit). Both are metrics worth plotting at S2.

## Known and deliberate

- **Collision is O(n²).** Correct and fast enough here (~6 300 ticks/s at 700
  entities, ~3 800× real time). A uniform-grid broadphase arrives with the
  terrain field at S1 and must iterate cells in index order to stay
  deterministic.
- **Demand vastly exceeds the bands at soak populations.** Earth with 294 bodies
  clips ~313 000 units per settlement against a ceiling of 1 400. The governor is
  doing its job, but it means that above a population threshold a race's marginal
  body changes the terrain by nothing at all. That caps the value of zerging a
  region — probably desirable — but it also decouples population from terrain
  influence, which is a real design question for S1, not a bug.
- **`speed` is an ecology knob, not a feel knob.** Mobility is the parameter
  that decides whether five biomes coexist in rotating spiral domains or
  collapse to a single survivor — there is a critical threshold and it is not
  where intuition puts it. Change it expecting the world to reorganise.
- **No terrain, no plants, no animals, no combat, no artifacts.** S1, S2 and S5
  respectively. Right now the only things to tune are the five race rows; plant
  and animal attributes, default behaviours, and abilities arrive with them.

## Next

S1 — the terrain field: five `u16` saturations per cell, six operators in fixed
order, climate influx map, filmstrip output, hot reload. Exit condition is
succession visibly cycling with no absorbing state over 30 simulated days.

## The client

`client/` is the Godot 4 project, carrying over the parts that decide how things
*look*: scenes, camera, input handling, the tap-outline shader, foliage layer,
transition overlay, and the web export config. Its tick rate already matches the
sim at 0.6 s.

It needs a significant overhaul before it renders this sim — it currently
expects the Node server that now lives in `legacy/`. The shape of that work:

1. Delete the remaining WebSocket assumptions; the client connects to a
   territory process, not a game server.
2. Render from terrain digests and entity streams rather than `TICK_STATE`
   position broadcasts.
3. Interpolate between sim ticks instead of advancing its own clock.

None of that is blocked on the sim — S1 and S2 come first, because there is
nothing worth rendering until terrain and ecology exist.
