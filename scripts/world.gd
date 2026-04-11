extends Node2D

@onready var tilemap: TileMapLayer = $TileMapLayer
@onready var player: Node2D = $Player

func _ready() -> void:
	_setup_tileset()
	_paint_test_grid()
	# Place the player at the center of the grid.
	player.set_tile_pos(Vector2i(5, 5))
	player.position = tilemap.map_to_local(Vector2i(5, 5))

func _setup_tileset() -> void:
	var tile_set := tilemap.tile_set
	# Add the collision_flags custom data layer.
	tile_set.add_custom_data_layer()
	tile_set.set_custom_data_layer_name(0, "collision_flags")
	tile_set.set_custom_data_layer_type(0, TYPE_INT)
	# Create a simple colored tile source (no texture needed).
	var source := TileSetAtlasSource.new()
	# Create a 1x1 pixel placeholder image as texture.
	var img := Image.create(32, 32, false, Image.FORMAT_RGBA8)
	img.fill(Color(0.3, 0.6, 0.2))  # Green grass color.
	var tex := ImageTexture.create_from_image(img)
	source.texture = tex
	source.texture_region_size = Vector2i(32, 32)
	tile_set.add_source(source, 0)
	# Create tile at atlas coords (0, 0).
	source.create_tile(Vector2i(0, 0))
	# Set collision_flags to 0 (fully walkable).
	var tile_data := source.get_tile_data(Vector2i(0, 0), 0)
	tile_data.set_custom_data("collision_flags", 0)

func _paint_test_grid() -> void:
	# Paint a 16x16 grid of walkable tiles.
	for x in range(16):
		for y in range(16):
			tilemap.set_cell(Vector2i(x, y), 0, Vector2i(0, 0))

func _unhandled_input(event: InputEvent) -> void:
	# Toggle run with R key.
	if event is InputEventKey:
		var key := event as InputEventKey
		if key.keycode == KEY_R and key.pressed and not key.echo:
			player.is_running = not player.is_running
			print("Running: ", player.is_running)
