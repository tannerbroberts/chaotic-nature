## Tap-scrim highlight overlay.
##
## Draws a white outline with a gray drop shadow around the "top-level" asset
## at a tapped tile position.  The hierarchy is:
##   foliage (highest)  >  base tile (lowest)
##
## The outline follows the asset's shape via an alpha-tracing shader
## (shaders/tap_outline.gdshader).  To give the shader room to draw outside the
## asset silhouette, the source texture is padded with transparent pixels before
## being assigned to the overlay sprite.
##
## See docs/foliage_architecture.md for full details.
extends Node2D

const HIGHLIGHT_PADDING := 8  # px of transparent padding for the outline to render into.

@export var foliage_path: NodePath

var _outline_sprite: Sprite2D
var _shader_material: ShaderMaterial
var _base_tilemap: TileMapLayer
var _foliage_layer: TileMapLayer
var _texture_cache: Dictionary = {}  # instance_id -> padded ImageTexture

func _ready() -> void:
	_foliage_layer = get_node_or_null(foliage_path) as TileMapLayer
	_base_tilemap = get_parent().get_node("TileMapLayer") as TileMapLayer
	_setup_overlay()

func _setup_overlay() -> void:
	_outline_sprite = Sprite2D.new()
	_outline_sprite.visible = false
	add_child(_outline_sprite)

	var shader := preload("res://shaders/tap_outline.gdshader")
	_shader_material = ShaderMaterial.new()
	_shader_material.shader = shader
	_outline_sprite.material = _shader_material

## Show the tap-scrim highlight on the top-level asset at [param tile_pos].
func highlight_at(tile_pos: Vector2i) -> void:
	# Foliage is highest priority; fall back to the base tile.
	if _foliage_layer and _foliage_layer.get_cell_source_id(tile_pos) != -1:
		var source := _foliage_layer.tile_set.get_source(0) as TileSetAtlasSource
		_show(tile_pos, source.texture)
	elif _base_tilemap and _base_tilemap.get_cell_source_id(tile_pos) != -1:
		var source := _base_tilemap.tile_set.get_source(0) as TileSetAtlasSource
		_show(tile_pos, source.texture)
	else:
		clear()

## Hide the highlight.
func clear() -> void:
	_outline_sprite.visible = false

# ---------------------------------------------------------------------------

func _show(tile_pos: Vector2i, original_tex: Texture2D) -> void:
	var padded := _get_padded_texture(original_tex)
	if padded == null:
		clear()
		return
	_outline_sprite.texture = padded
	_outline_sprite.position = _base_tilemap.map_to_local(tile_pos)
	_outline_sprite.visible = true

func _get_padded_texture(tex: Texture2D) -> ImageTexture:
	var key := tex.get_instance_id()
	if key in _texture_cache:
		return _texture_cache[key]

	var img := tex.get_image()
	if img == null:
		return null

	# Decompress if needed so blit_rect works.
	if img.is_compressed():
		img.decompress()

	var w := img.get_width() + HIGHLIGHT_PADDING * 2
	var h := img.get_height() + HIGHLIGHT_PADDING * 2
	var padded := Image.create(w, h, false, Image.FORMAT_RGBA8)
	padded.blit_rect(
		img,
		Rect2i(0, 0, img.get_width(), img.get_height()),
		Vector2i(HIGHLIGHT_PADDING, HIGHLIGHT_PADDING)
	)
	var result := ImageTexture.create_from_image(padded)
	_texture_cache[key] = result
	return result
