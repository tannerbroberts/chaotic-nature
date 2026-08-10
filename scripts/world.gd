extends Node2D

const PlayerScene = preload("res://scenes/player.tscn")
var LobbyScene: PackedScene = load("res://scenes/lobby.tscn")
const TransitionOverlayScript = preload("res://scripts/transition_overlay.gd")
const POIActionMenuScript = preload("res://scripts/poi_action_menu.gd")

@onready var tilemap: TileMapLayer = $TileMapLayer
@onready var player: Node2D = $Player

var _run_button: Button
var _leave_button: Button
var _room_label: Label
var _remote_players: Dictionary = {}  # player_id (int) -> Node2D
var _transition_overlay: CanvasLayer
var _poi_tiles: Array[Vector2i] = []
var _poi_exit_map: Dictionary = {}  # Vector2i (poi tile) -> String (exit direction)
var _poi_action_menu: Node2D

# Room context — set by lobby before adding to tree.
var room_id: String = "0,0"
var room_spawn: Vector2i = Vector2i(5, 5)
var room_players: Array[Dictionary] = []

func _ready() -> void:
	_setup_tileset()
	_paint_test_grid()
	_compute_poi_tiles()
	_paint_poi_tiles()
	# Place the player at the server-assigned spawn.
	player.set_tile_pos(room_spawn)
	player.position = tilemap.map_to_local(room_spawn)
	_create_ui()
	_create_transition_overlay()
	_create_poi_action_menu()
	# Spawn existing players already in the room.
	for p: Dictionary in room_players:
		var pid: int = p["player_id"]
		if pid != NetworkManager.my_player_id:
			_spawn_remote(pid, p["tile_x"], p["tile_y"])
	# Connect network signals.
	NetworkManager.tick_state_received.connect(_on_tick_state)
	NetworkManager.player_joined.connect(_on_player_joined)
	NetworkManager.player_left.connect(_on_player_left)
	NetworkManager.room_transfer_started.connect(_on_room_transfer)
	NetworkManager.room_joined.connect(_on_room_joined)
	NetworkManager.room_list_received.connect(_on_room_list)
	NetworkManager.disconnected.connect(_on_disconnected)

func _setup_tileset() -> void:
	var tile_set := tilemap.tile_set
	# Add the collision_flags custom data layer.
	tile_set.add_custom_data_layer()
	tile_set.set_custom_data_layer_name(0, "collision_flags")
	tile_set.set_custom_data_layer_type(0, TYPE_INT)
	# Create a two-tile atlas for a subtle checkerboard pattern.
	var base_color := Color(0.3, 0.6, 0.2)  # Green grass color.
	var tint := 0.05
	var color_a := base_color.darkened(tint)
	var color_b := base_color.lightened(tint)
	# Build a 128x64 atlas image: tile A on the left, tile B on the right.
	var img := Image.create(128, 64, false, Image.FORMAT_RGBA8)
	img.fill_rect(Rect2i(0, 0, 64, 64), color_a)
	img.fill_rect(Rect2i(64, 0, 64, 64), color_b)
	var tex := ImageTexture.create_from_image(img)
	var source := TileSetAtlasSource.new()
	source.texture = tex
	source.texture_region_size = Vector2i(64, 64)
	tile_set.add_source(source, 0)
	# Tile A at atlas coords (0, 0).
	source.create_tile(Vector2i(0, 0))
	var tile_data_a := source.get_tile_data(Vector2i(0, 0), 0)
	tile_data_a.set_custom_data("collision_flags", 0)
	# Tile B at atlas coords (1, 0).
	source.create_tile(Vector2i(1, 0))
	var tile_data_b := source.get_tile_data(Vector2i(1, 0), 0)
	tile_data_b.set_custom_data("collision_flags", 0)

func _paint_test_grid() -> void:
	# Paint a 16x16 grid of walkable tiles with a checkerboard pattern.
	for x in range(16):
		for y in range(16):
			var atlas_col := (x + y) % 2  # Alternate between tile A and B.
			tilemap.set_cell(Vector2i(x, y), 0, Vector2i(atlas_col, 0))

func _process(_delta: float) -> void:
	# Show/hide POI action menu based on player tile position.
	if _poi_action_menu and _poi_tiles.has(player.tile_pos):
		if not _poi_action_menu.visible:
			_poi_action_menu.show_at(tilemap.map_to_local(player.tile_pos))
	elif _poi_action_menu and _poi_action_menu.visible:
		_poi_action_menu.hide_menu()

func _compute_poi_tiles() -> void:
	_poi_tiles.clear()
	var parts := room_id.split(",")
	var gx := int(parts[0])
	var gy := int(parts[1])
	# Three POI tiles per edge: the exit tile itself + one on each side.
	_poi_exit_map.clear()
	if gx < 1:  # East neighbor exists
		_poi_tiles.append(Vector2i(15, 7))
		_poi_tiles.append(Vector2i(15, 8))
		_poi_tiles.append(Vector2i(15, 9))
		_poi_exit_map[Vector2i(15, 7)] = "east"
		_poi_exit_map[Vector2i(15, 8)] = "east"
		_poi_exit_map[Vector2i(15, 9)] = "east"
	if gx > 0:  # West neighbor exists
		_poi_tiles.append(Vector2i(0, 7))
		_poi_tiles.append(Vector2i(0, 8))
		_poi_tiles.append(Vector2i(0, 9))
		_poi_exit_map[Vector2i(0, 7)] = "west"
		_poi_exit_map[Vector2i(0, 8)] = "west"
		_poi_exit_map[Vector2i(0, 9)] = "west"
	if gy < 1:  # South neighbor exists
		_poi_tiles.append(Vector2i(7, 15))
		_poi_tiles.append(Vector2i(8, 15))
		_poi_tiles.append(Vector2i(9, 15))
		_poi_exit_map[Vector2i(7, 15)] = "south"
		_poi_exit_map[Vector2i(8, 15)] = "south"
		_poi_exit_map[Vector2i(9, 15)] = "south"
	if gy > 0:  # North neighbor exists
		_poi_tiles.append(Vector2i(7, 0))
		_poi_tiles.append(Vector2i(8, 0))
		_poi_tiles.append(Vector2i(9, 0))
		_poi_exit_map[Vector2i(7, 0)] = "north"
		_poi_exit_map[Vector2i(8, 0)] = "north"
		_poi_exit_map[Vector2i(9, 0)] = "north"

func _paint_poi_tiles() -> void:
	var tile_set := tilemap.tile_set
	var source := tile_set.get_source(0) as TileSetAtlasSource
	if not source.has_tile(Vector2i(2, 0)):
		var old_tex := source.texture
		var old_img := old_tex.get_image()
		var old_width := old_img.get_width()
		var new_img := Image.create(192, 64, false, Image.FORMAT_RGBA8)
		new_img.blit_rect(old_img, Rect2i(0, 0, old_width, 64), Vector2i.ZERO)
		var poi_color := Color(0.25, 0.55, 0.55)
		new_img.fill_rect(Rect2i(128, 0, 64, 64), poi_color)
		var new_tex := ImageTexture.create_from_image(new_img)
		source.texture = new_tex
		source.create_tile(Vector2i(2, 0))
		var poi_data := source.get_tile_data(Vector2i(2, 0), 0)
		poi_data.set_custom_data("collision_flags", 0)
	for tile: Vector2i in _poi_tiles:
		tilemap.set_cell(tile, 0, Vector2i(2, 0))

func _create_poi_action_menu() -> void:
	_poi_action_menu = POIActionMenuScript.new()
	_poi_action_menu.button_pressed.connect(_on_poi_button_pressed)
	add_child(_poi_action_menu)

func _on_poi_button_pressed(index: int) -> void:
	print("POI action button pressed: ", index)
	if index == 0:
		# Center button: request transfer to the adjacent territory.
		var current_poi: Vector2i = player.tile_pos
		if current_poi in _poi_exit_map:
			var direction: String = _poi_exit_map[current_poi]
			NetworkManager.send_transfer_request(direction)

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

func _create_ui() -> void:
	var canvas := CanvasLayer.new()
	canvas.layer = 10
	add_child(canvas)

	# Room label (top-left).
	_room_label = Label.new()
	_room_label.text = "Territory (%s)" % room_id
	_room_label.add_theme_font_size_override("font_size", 16)
	_room_label.add_theme_color_override("font_color", Color(0.8, 0.9, 0.7))
	_room_label.anchor_left = 0.0
	_room_label.anchor_top = 0.0
	_room_label.offset_left = 10
	_room_label.offset_top = 10
	canvas.add_child(_room_label)

	# Run button (bottom-right).
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

	# Leave button (top-right).
	_leave_button = Button.new()
	_leave_button.text = "Leave"
	_leave_button.anchor_left = 1.0
	_leave_button.anchor_top = 0.0
	_leave_button.anchor_right = 1.0
	_leave_button.anchor_bottom = 0.0
	_leave_button.offset_left = -90
	_leave_button.offset_top = 10
	_leave_button.offset_right = -10
	_leave_button.offset_bottom = 50
	_leave_button.add_theme_font_size_override("font_size", 16)
	_leave_button.pressed.connect(_on_leave_pressed)
	canvas.add_child(_leave_button)

func _create_transition_overlay() -> void:
	_transition_overlay = TransitionOverlayScript.new()
	add_child(_transition_overlay)

func _on_run_toggled(pressed: bool) -> void:
	player.is_running = pressed
	if NetworkManager.my_player_id >= 0:
		NetworkManager.send_toggle_run(pressed)
	print("Running: ", pressed)

func _on_leave_pressed() -> void:
	NetworkManager.send_leave_room()
	_return_to_lobby()

# ── Network callbacks ────────────────────────────────────────────────────────

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

func _on_room_transfer(dest_room_id: String, _transfer_time_ms: int) -> void:
	print("Transferring to room ", dest_room_id)
	_transition_overlay.show_transfer(dest_room_id)
	if _poi_action_menu:
		_poi_action_menu.hide_menu()
	# Clear local state — we're in transit.
	for pid: int in _remote_players:
		(_remote_players[pid] as Node2D).queue_free()
	_remote_players.clear()

func _on_room_joined(new_room_id: String, spawn_x: int, spawn_y: int, players: Array[Dictionary]) -> void:
	print("Joined room ", new_room_id, " at (", spawn_x, ",", spawn_y, ")")
	# Update room context.
	room_id = new_room_id
	room_spawn = Vector2i(spawn_x, spawn_y)
	_room_label.text = "Territory (%s)" % room_id
	# Clear old remote players (should already be clear from transfer).
	for pid: int in _remote_players:
		(_remote_players[pid] as Node2D).queue_free()
	_remote_players.clear()
	# Re-paint tiles for the new room.
	_paint_test_grid()
	_compute_poi_tiles()
	_paint_poi_tiles()
	# Place player at new spawn.
	player.set_tile_pos(room_spawn)
	player.position = tilemap.map_to_local(room_spawn)
	# Spawn existing players.
	for p: Dictionary in players:
		var pid: int = p["player_id"]
		if pid != NetworkManager.my_player_id:
			_spawn_remote(pid, p["tile_x"], p["tile_y"])
	# Hide transition overlay.
	_transition_overlay.hide_transfer()

func _on_room_list(_rooms: Array[Dictionary]) -> void:
	# In-game we ignore room list updates (lobby-only concern).
	pass

func _on_disconnected() -> void:
	_return_to_lobby()

func _return_to_lobby() -> void:
	# Disconnect signals to avoid calling into freed nodes.
	if NetworkManager.tick_state_received.is_connected(_on_tick_state):
		NetworkManager.tick_state_received.disconnect(_on_tick_state)
	if NetworkManager.player_joined.is_connected(_on_player_joined):
		NetworkManager.player_joined.disconnect(_on_player_joined)
	if NetworkManager.player_left.is_connected(_on_player_left):
		NetworkManager.player_left.disconnect(_on_player_left)
	if NetworkManager.room_transfer_started.is_connected(_on_room_transfer):
		NetworkManager.room_transfer_started.disconnect(_on_room_transfer)
	if NetworkManager.room_joined.is_connected(_on_room_joined):
		NetworkManager.room_joined.disconnect(_on_room_joined)
	if NetworkManager.room_list_received.is_connected(_on_room_list):
		NetworkManager.room_list_received.disconnect(_on_room_list)
	if NetworkManager.disconnected.is_connected(_on_disconnected):
		NetworkManager.disconnected.disconnect(_on_disconnected)
	var lobby := LobbyScene.instantiate()
	get_tree().root.add_child(lobby)
	queue_free()
