# Foliage Layer & Tap-Scrim Highlight Architecture

## Overview

Two systems added on top of the existing tile-based movement architecture:

1. **Foliage TileMapLayer** — a decoration layer for plant assets (ferns, etc.)
2. **Tap-Scrim Highlight** — a white-outline + gray-shadow border shown around
   whichever asset is "top-level" at the tapped tile position.

Both are designed for mobile-first play.

---

## Foliage Layer

### Scene node

`FoliageLayer` — `TileMapLayer` (child of World, sibling of base `TileMapLayer`)

**File:** `scripts/foliage_layer.gd`

**Why a second TileMapLayer:** Decoration must sit above terrain in z-order but
share the same 32×32 grid so that coordinate conversion (`local_to_map` /
`map_to_local`) works identically to the base layer.  Using a TileMapLayer (vs.
free-floating Sprite2D nodes) keeps batch-drawing efficient on mobile GPUs and
lets the pathfinder and input handler reference foliage by tile coordinate.

### Tile size vs. asset size

The foliage TileSet has `tile_size = 32×32` (same grid), but the atlas source's
`texture_region_size` is `48×48`.  The 48 px fern texture centers on the 32 px
cell and overflows by 8 px on each side.  This produces visual overlap between
adjacent tiles — intentional for a natural look.

### Asset border rule

> **Every foliage asset MUST include an outer border (stroke) on all filled
> shapes.**

The tap-highlight shader traces the alpha boundary of an asset to draw its scrim
outline.  If the asset has soft, semi-transparent edges without a crisp stroke,
the shader samples bleed and the outline flickers.  Baking a 1 px stroke into
the SVG guarantees a clean alpha edge.

See `assets/foliage/fern.svg` for the reference implementation.

### Adding a new foliage asset

1. Create the SVG in `assets/foliage/`.  Use a viewBox larger than 32×32 if the
   asset should visually exceed one tile.
2. Add a border stroke to **every** filled path — no exceptions.
3. In `foliage_layer.gd`, add a new atlas source (unique source ID) with
   `texture_region_size` matching the SVG dimensions.
4. Create a tile on the source and place it with `set_cell()`.

---

## Tap-Scrim Highlight

### Scene node

`TapHighlight` — `Node2D` (child of World, between FoliageLayer and Player)

**File:** `scripts/tap_highlight.gd`

### Top-level hierarchy

When the player taps a tile, the highlight selects the **highest-priority** asset
present at that coordinate:

| Priority | Layer            | Example |
|----------|------------------|---------|
| 1 (high) | FoliageLayer     | Fern    |
| 0 (base) | TileMapLayer     | Grass   |

If foliage exists at the tapped tile, the fern gets the highlight.  Otherwise the
base terrain tile does.  Future layers (structures, items) slot into the priority
list by adding more checks in `highlight_at()`.

### Outline rendering

**Shader:** `shaders/tap_outline.gdshader`

The shader runs on a `Sprite2D` child of TapHighlight.  It samples the texture's
alpha channel in 16 evenly-spaced directions at two radii:

- **Outline ring** (`outline_width` = 2 px) — white, full opacity.
- **Shadow ring** (`shadow_width` = 3.5 px, offset 1.5 px down-right) — gray,
  60 % opacity.

Pixels inside the asset silhouette are forced transparent so the original asset
shows through beneath the overlay.

### Texture padding

The outline extends *outside* the asset silhouette.  Because shaders can only
color fragments within the sprite quad, the source texture is padded with 8 px of
transparency on each side before being assigned to the overlay sprite.  Padded
textures are cached by instance ID so the Image manipulation only happens once
per unique asset.

### Integration with InputHandler

`input_handler.gd` holds an `@export var tap_highlight_path` pointing at the
TapHighlight node.  On every tap (mouse or touch), it calls
`_tap_highlight.highlight_at(clicked_tile)` before issuing pathfinding.  Movement
and highlighting are independent — the player moves even if the highlight lands
on foliage.

---

## File inventory

| Path | Purpose |
|------|---------|
| `assets/foliage/fern.svg` | 48×48 fern SVG with border strokes |
| `scripts/foliage_layer.gd` | Foliage TileMapLayer setup & placement |
| `scripts/tap_highlight.gd` | Tap-scrim outline overlay logic |
| `shaders/tap_outline.gdshader` | Alpha-boundary outline + shadow shader |
| `docs/foliage_architecture.md` | This document |
