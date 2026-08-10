# Movement System Architecture

## Overview

This document describes the OSRS-style tile-based movement system for Chaotic Nature.
All game logic runs on a **0.6-second tick** cycle. Players move by tapping a tile on
screen; a BFS pathfinder computes the route, and movement consumes 1 tile/tick (walk)
or 2 tiles/tick (run).

---

## OSRS Movement Mechanics Reference

| Mechanic | Detail |
|---|---|
| Game tick | 0.6 seconds. All server-style logic fires once per tick. |
| Walking speed | 1 tile per tick |
| Running speed | 2 tiles per tick (two 1-tile steps consumed from the queue) |
| Distance metric | Chebyshev (diagonal = cardinal cost). 8-directional movement. |
| Pathfinding | BFS on a 2D tile grid, respecting per-tile collision flags. |
| Walk queue | Ordered list of tile coords. Click replaces the queue. Each tick, 1 or 2 entries are dequeued. |
| Collision flags | Bitwise int per tile encoding which of 8 directions are blocked (walls, objects, terrain). |
| Visual smoothing | Logical position snaps to tiles; visual position interpolates between ticks for smooth rendering. |

---

## Scene / Node Architecture

### 1. GameTickManager — `Node` (Autoload singleton)

**File:** `scripts/game_tick_manager.gd`

**Why an autoload:** The tick is the heartbeat of the entire game. Every system —
movement, future combat, future skilling — must listen to the same clock. Making it
an autoload ensures a single source of truth accessible from anywhere, with no
dependency on scene tree structure.

**Why a signal, not _process:** Using a `tick` signal decouples consumers from the
timer implementation. Systems subscribe to `GameTickManager.tick` rather than
checking elapsed time themselves, preventing drift between systems.

**Responsibility:**
- Maintain a 0.6s timer.
- Emit `tick` signal each cycle.
- Expose a `tick_count` for sequencing / debugging.

---

### 2. World — `Node2D` (Root scene)

**File:** `scenes/world.tscn`

**Why a dedicated root scene:** The World owns the tile map, all entities (player,
future NPCs), the camera, and the input handler. Grouping them here means loading a
level is loading one scene. It also provides a single coordinate space — every child
uses the same local-to-world transform.

**Children:**
- `TileMapLayer` — the grid
- `Player` — the player entity
- `InputHandler` — tap-to-tile conversion
- `Camera2D` — smooth-follow camera

---

### 3. TileMapLayer — `TileMapLayer` (Godot built-in)

**Why TileMapLayer:** Godot 4.4+ TileMapLayer provides `local_to_map()` and
`map_to_local()` for free coordinate conversion between pixel and grid space. Custom
data layers let us store `collision_flags` (int) per tile without a parallel data
structure. The renderer handles tile drawing automatically.

**Custom data:**
- `collision_flags` (int) — bitwise flags encoding blocked directions per tile.
  Bit layout: `N=1, NE=2, E=4, SE=8, S=16, SW=32, W=64, NW=128`.

---

### 4. Player — `Node2D` + child `Sprite2D`

**File:** `scenes/player.tscn`, `scripts/player.gd`

**Why Node2D (not CharacterBody2D):** We don't use Godot's physics movement. All
movement is tile-discrete and tick-driven. Node2D is the simplest base that gives us
a transform. CharacterBody2D's `move_and_slide` would fight our system.

**Why separate logical vs. visual position:** The logical position (`tile_pos: Vector2i`)
snaps instantly each tick. The visual `position` property interpolates toward the
logical position in `_process`. This replicates the OSRS feel: the game state is
discrete, but rendering is smooth.

**Walk queue:** An `Array[Vector2i]` of tile coordinates. Each tick:
- Dequeue 1 tile (walking) or 2 tiles (running).
- Update `tile_pos`.
- Visual interpolation target updates to `tilemap.map_to_local(tile_pos)`.

**Why the queue replaces on new click:** OSRS cancels the current path and starts a
new one when you click. This keeps input responsive — no queuing of multiple
destinations.

---

### 5. Pathfinder — `RefCounted` class (pure script, no node)

**File:** `scripts/pathfinder.gd`

**Why not a node:** Pathfinding is pure computation: given a grid, a start, and an
end, return a path. It has no per-frame behavior, no children, no rendering. Making
it a RefCounted class keeps it out of the scene tree, makes it easily testable, and
lets both the player and future NPCs share the same instance.

**Algorithm:** BFS (breadth-first search). BFS guarantees the shortest path on an
unweighted grid, which matches OSRS behavior. The search respects collision flags:
before expanding into an adjacent tile, the pathfinder checks both the exit flags of
the current tile and the entry flags of the neighbor.

**Why BFS over A*:** On a small-to-medium tile grid with uniform cost, BFS is simpler
and produces identical results to A* with a constant heuristic. OSRS itself uses
BFS-like expansion.

---

### 6. InputHandler — `Node` (child of World)

**File:** `scripts/input_handler.gd`

**Why a separate node:** Decoupling input from the Player means the Player scene is
a pure movement entity. NPCs can reuse the same Player movement logic without input.
It also centralizes input processing — future features like right-click menus or UI
blocking only need to modify this one node.

**Responsibility:**
- Listen for screen tap / mouse click.
- Convert screen position → world position → tile coordinate via TileMapLayer.
- Call Pathfinder with player's current tile and clicked tile.
- Set the Player's walk queue to the returned path.
- Handle walk/run toggle.

---

### 7. Camera2D — `Camera2D` (child of World, NOT child of Player)

**Why not parented to Player:** The Player's `position` is the *visual* interpolated
position. If the camera were a child, it would be correct. But keeping it as a
sibling in World and following the player's visual position via script gives us more
control — e.g., camera boundaries, future cutscenes, or smooth transitions when
teleporting.

**Responsibility:**
- Each frame, lerp toward the Player's visual position.
- Clamp to world bounds if needed.
