extends Node2D

@onready var tilemap: TileMapLayer = $TileMapLayer
@onready var player: Node2D = $Player

var _run_button: Button

func _ready() -> void:
	_setup_tileset()
	_paint_test_grid()
	# Place the player at the center of the grid.
	player.set_tile_pos(Vector2i(5, 5))
	player.position = tilemap.map_to_local(Vector2i(5, 5))
	_create_run_button()

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
			_run_button.button_pressed = player.is_running
			print("Running: ", player.is_running)

func _create_run_button() -> void:
	var canvas := CanvasLayer.new()
	canvas.layer = 10
	add_child(canvas)

	_run_button = Button.new()
	_run_button.text = "Run"
	_run_button.toggle_mode = true
	_run_button.anchor_left = 1.0
	_run_button.anchor_top = 1.0
	_run_button.anchor_right = 1.0
	_run_button.anchor_bottom = 1.0
	_run_button.offset_left = -100
	_run_button.offset_top = -60
	_run_button.offset_right = -10
	_run_button.offset_bottom = -10
	_run_button.add_theme_font_size_override("font_size", 20)
	_run_button.toggled.connect(_on_run_toggled)
	canvas.add_child(_run_button)

func _on_run_toggled(pressed: bool) -> void:
	player.is_running = pressed
	print("Running: ", pressed)
