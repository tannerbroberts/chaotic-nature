## Foliage decoration layer — manages a TileMapLayer for foliage assets.
##
## Architecture: Foliage uses a separate TileMapLayer on the same 32×32 grid as
## the base terrain.  Assets may exceed the tile boundaries (the fern is 48×48)
## but always center on a single tile.
##
## BORDER RULE: Every foliage asset MUST include an outer border stroke.  The
## tap-highlight shader traces the alpha boundary to produce a scrim outline;
## without a clean border, the outline flickers on semi-transparent edges.
##
## See docs/foliage_architecture.md for full details.
extends TileMapLayer

const FERN_SOURCE_ID := 0
const FERN_ATLAS_COORD := Vector2i(0, 0)

func _ready() -> void:
	_setup_tileset()
	_place_test_foliage()

func _setup_tileset() -> void:
	var ts := tile_set  # TileSet assigned in the scene (tile_size 32×32).
	var fern_tex: Texture2D = preload("res://assets/foliage/fern.svg")
	var source := TileSetAtlasSource.new()
	source.texture = fern_tex
	source.texture_region_size = Vector2i(48, 48)
	ts.add_source(source, FERN_SOURCE_ID)
	source.create_tile(FERN_ATLAS_COORD)

func _place_test_foliage() -> void:
	# Scatter a handful of ferns across the grid for visual testing.
	var positions := [
		Vector2i(3, 4), Vector2i(7, 2), Vector2i(10, 8),
		Vector2i(5, 12), Vector2i(13, 6),
	]
	for pos in positions:
		set_cell(pos, FERN_SOURCE_ID, FERN_ATLAS_COORD)

## Returns true if there is any foliage asset at the given tile coordinate.
func has_foliage(tile: Vector2i) -> bool:
	return get_cell_source_id(tile) != -1
