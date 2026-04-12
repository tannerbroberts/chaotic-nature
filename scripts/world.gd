extends Node2D

const PlayerScene = preload("res://scenes/player.tscn")

@onready var tilemap: TileMapLayer = $TileMapLayer
@onready var player: Node2D = $Player

var _run_button: Button
var _remote_players: Dictionary = {}  # player_id (int) -> Node2D

func _ready() -> void:
	_setup_tileset()
	_paint_test_grid()
	# Place the player at the center of the grid.
	player.set_tile_pos(Vector2i(5, 5))
	player.position = tilemap.map_to_local(Vector2i(5, 5))
	_create_run_button()
	# Connect to the game server.
	NetworkManager.welcome_received.connect(_on_welcome)
	NetworkManager.tick_state_received.connect(_on_tick_state)
	NetworkManager.player_joined.connect(_on_player_joined)
	NetworkManager.player_left.connect(_on_player_left)
	NetworkManager.connect_to_server()

func _setup_tileset() -> void:
	var tile_set := tilemap.tile_set
	# Add the collision_flags custom data layer.
	tile_set.add_custom_data_layer()
	tile_set.set_custom_data_layer_name(0, "collision_flags")
	tile_set.set_custom_data_layer_type(0, TYPE_INT)
	# Create a simple colored tile source (no texture needed).
	var source := TileSetAtlasSource.new()
	# Create a 1x1 pixel placeholder image as texture.
	var img := Image.create(64, 64, false, Image.FORMAT_RGBA8)
	img.fill(Color(0.3, 0.6, 0.2))  # Green grass color.
	var tex := ImageTexture.create_from_image(img)
	source.texture = tex
	source.texture_region_size = Vector2i(64, 64)
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
			if NetworkManager.my_player_id >= 0:
				NetworkManager.send_toggle_run(player.is_running)
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
	if NetworkManager.my_player_id >= 0:
		NetworkManager.send_toggle_run(pressed)
	print("Running: ", pressed)

# ── Network callbacks ────────────────────────────────────────────────────────

func _on_welcome(player_id: int, tile_x: int, tile_y: int) -> void:
	player.set_tile_pos(Vector2i(tile_x, tile_y))
	player.position = tilemap.map_to_local(Vector2i(tile_x, tile_y))
	print("Welcome: player_id=", player_id, " at (", tile_x, ",", tile_y, ")")

func _on_tick_state(_tick_number: int, players: Array[Dictionary]) -> void:
	for p: Dictionary in players:
		var pid: int = p["player_id"]
		var tile := Vector2i(p["tile_x"], p["tile_y"])
		if pid == NetworkManager.my_player_id:
			player.set_tile_pos(tile)
		elif pid in _remote_players:
			(_remote_players[pid] as Node2D).set_tile_pos(tile)
		else:
			_spawn_remote(pid, tile.x, tile.y)

func _on_player_joined(player_id: int, tile_x: int, tile_y: int) -> void:
	if player_id == NetworkManager.my_player_id:
		return
	if player_id not in _remote_players:
		_spawn_remote(player_id, tile_x, tile_y)

func _on_player_left(player_id: int) -> void:
	if player_id in _remote_players:
		(_remote_players[player_id] as Node2D).queue_free()
		_remote_players.erase(player_id)
		print("Player ", player_id, " left")

func _spawn_remote(player_id: int, tile_x: int, tile_y: int) -> void:
	var remote := PlayerScene.instantiate()
	remote.tilemap_path = remote.get_path_to(tilemap)
	add_child(remote)
	# tilemap_path must resolve after adding to tree, so set it directly.
	remote.tilemap = tilemap
	var tile := Vector2i(tile_x, tile_y)
	remote.set_tile_pos(tile)
	remote.position = tilemap.map_to_local(tile)
	# Tint remote players a different colour.
	var sprite := remote.get_node("Sprite2D") as Sprite2D
	if sprite:
		sprite.modulate = Color(0.9, 0.3, 0.2)  # Red-ish
	_remote_players[player_id] = remote
	print("Player ", player_id, " joined at (", tile_x, ",", tile_y, ")")
