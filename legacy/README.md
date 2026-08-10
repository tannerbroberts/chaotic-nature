# legacy/

Everything here decided **what is true** in the pre-consolidation architecture:
tick authority, pathfinding, and the wire protocol. All of it is superseded by
the Rust simulation at the repository root.

Nothing in this directory is referenced by anything that builds. It is kept
because the designs are worth reading while the Rust equivalents get written,
and it is deletable in a single commit once S3 lands.

## What replaced what

| Superseded | Replaced by | Why |
|---|---|---|
| `server/game_server.js` | `src/world.rs` — `World::step` | The tick clock and all authority moved into the deterministic sim. |
| `server/pathfinder.js`, `pathfinder.gd` | `src/world.rs` — `phase_movement` | Movement is simulated, not pathfound. A client can predict it *exactly* by running the same deterministic step, so a reimplemented BFS is not needed. |
| `game_tick_manager.gd` | `src/world.rs` — sim tick | Godot must not own a tick clock. Its job is to interpolate between authoritative states, not to advance them. |
| `server/protocol.json`, `codec.js`, `network/codec.gd` | *(to be written at S3)* | See the note below — this one is not a straight port. |
| `network/network_manager.gd` | *(to be written at S3)* | Pure plumbing for the protocol above; dies with it. |
| `movement_architecture.md` | `docs/` + the design doc | Describes tile-based movement under Node authority. |

## Why the protocol is a redesign, not a port

The TLV framing in `protocol.json` is good and worth keeping — a `u8` type, a
`u16` length, and unknown types skipped via the length field for forward
compatibility. Reuse that shape.

What cannot carry over is `TICK_STATE`, which broadcasts every player's position
every tick. The consolidated architecture's Invariant V is **inputs replicate,
state does not**: territories exchange input deltas and coarse terrain digests,
and each one re-simulates. That is what makes seamless territory boundaries
possible at all — two servers agree because they computed the same thing from
the same inputs, not because one told the other what the answer was.

So the S3 protocol keeps the framing and replaces the message set:

```
superseded                        replacement shape
──────────                        ─────────────────
MOVE_TO    client → server        input delta, stamped for tick T + L
TOGGLE_RUN client → server        input delta, stamped for tick T + L
TICK_STATE server → client   ✗    never — state is not shipped for correctness
                             +    terrain digest, 32×32×5 u8, once per terrain tick
                             +    band state hash, per sim tick, between neighbours
```

One thing that did carry over unchanged: the 0.6 s tick. The Godot project and
the sim already agree on it.

## Restoring any of this

```
git log --oneline main          # eddf481 is the last pre-consolidation commit
git show eddf481:server/game_server.js
```

`legacy/exports/` (36 MB of Godot web build output) and
`legacy/server/node_modules/` are still on disk but no longer tracked. They were
tracked before the consolidation; history still has them.
