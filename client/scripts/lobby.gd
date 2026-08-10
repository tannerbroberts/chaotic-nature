extends Control
## Lobby — Default view after connecting. Shows a 2×2 grid of room panels
## with live player counts. Click a panel to join that room.

var WorldScene: PackedScene = load("res://scenes/world.tscn")

var _panels: Dictionary = {}  # room_id (String) -> Panel
var _count_labels: Dictionary = {}  # room_id (String) -> Label
var _grid: GridContainer
var _status_label: Label
var _title_label: Label

func _ready() -> void:
	_build_ui()
	NetworkManager.welcome_received.connect(_on_welcome)
	NetworkManager.room_list_received.connect(_on_room_list)
	NetworkManager.room_joined.connect(_on_room_joined)
	NetworkManager.disconnected.connect(_on_disconnected)
	if NetworkManager.is_connected_to_server():
		_status_label.text = "Connected — choose a territory"
	else:
		NetworkManager.connect_to_server()

func _build_ui() -> void:
	# Background color.
	var bg := ColorRect.new()
	bg.color = Color(0.12, 0.14, 0.10)
	bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(bg)

	# Vertical layout.
	var vbox := VBoxContainer.new()
	vbox.set_anchors_preset(Control.PRESET_CENTER)
	vbox.offset_left = -200
	vbox.offset_right = 200
	vbox.offset_top = -180
	vbox.offset_bottom = 180
	vbox.add_theme_constant_override("separation", 16)
	add_child(vbox)

	# Title.
	_title_label = Label.new()
	_title_label.text = "Chaotic Nature"
	_title_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_title_label.add_theme_font_size_override("font_size", 28)
	_title_label.add_theme_color_override("font_color", Color(0.8, 0.9, 0.7))
	vbox.add_child(_title_label)

	# Subtitle.
	var sub := Label.new()
	sub.text = "Select a Territory"
	sub.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	sub.add_theme_font_size_override("font_size", 16)
	sub.add_theme_color_override("font_color", Color(0.6, 0.7, 0.5))
	vbox.add_child(sub)

	# 2×2 grid of room buttons.
	_grid = GridContainer.new()
	_grid.columns = 2
	_grid.add_theme_constant_override("h_separation", 12)
	_grid.add_theme_constant_override("v_separation", 12)
	vbox.add_child(_grid)

	# Create placeholder panels for the 4 rooms.
	var room_ids := ["0,0", "1,0", "0,1", "1,1"]
	for rid: String in room_ids:
		var panel := _create_room_panel(rid, 0)
		_grid.add_child(panel)

	# Status label at the bottom.
	_status_label = Label.new()
	_status_label.text = "Connecting..."
	_status_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_status_label.add_theme_font_size_override("font_size", 14)
	_status_label.add_theme_color_override("font_color", Color(0.5, 0.5, 0.5))
	vbox.add_child(_status_label)

func _create_room_panel(room_id: String, player_count: int) -> Button:
	var btn := Button.new()
	btn.custom_minimum_size = Vector2(185, 120)
	btn.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND

	# Style the button.
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.2, 0.35, 0.18)
	style.border_color = Color(0.4, 0.55, 0.3)
	style.set_border_width_all(2)
	style.set_corner_radius_all(6)
	style.set_content_margin_all(12)
	btn.add_theme_stylebox_override("normal", style)

	var hover := style.duplicate() as StyleBoxFlat
	hover.bg_color = Color(0.25, 0.42, 0.22)
	hover.border_color = Color(0.5, 0.65, 0.4)
	btn.add_theme_stylebox_override("hover", hover)

	var pressed := style.duplicate() as StyleBoxFlat
	pressed.bg_color = Color(0.15, 0.28, 0.12)
	btn.add_theme_stylebox_override("pressed", pressed)

	# Layout inside the button: use a VBoxContainer.
	var inner := VBoxContainer.new()
	inner.mouse_filter = Control.MOUSE_FILTER_IGNORE
	inner.set_anchors_preset(Control.PRESET_FULL_RECT)
	inner.add_theme_constant_override("separation", 4)
	btn.add_child(inner)

	var title := Label.new()
	title.text = "Territory (%s)" % room_id
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	title.add_theme_font_size_override("font_size", 16)
	title.add_theme_color_override("font_color", Color(0.9, 0.95, 0.85))
	title.mouse_filter = Control.MOUSE_FILTER_IGNORE
	inner.add_child(title)

	var coords := Label.new()
	var parts := room_id.split(",")
	coords.text = "Grid [%s, %s]" % [parts[0], parts[1]]
	coords.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	coords.add_theme_font_size_override("font_size", 12)
	coords.add_theme_color_override("font_color", Color(0.6, 0.7, 0.5))
	coords.mouse_filter = Control.MOUSE_FILTER_IGNORE
	inner.add_child(coords)

	var count := Label.new()
	count.text = "%d players" % player_count
	count.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	count.add_theme_font_size_override("font_size", 14)
	count.add_theme_color_override("font_color", Color(0.7, 0.8, 0.6))
	count.mouse_filter = Control.MOUSE_FILTER_IGNORE
	inner.add_child(count)

	btn.pressed.connect(_on_room_pressed.bind(room_id))

	_panels[room_id] = btn
	_count_labels[room_id] = count
	return btn

func _on_room_pressed(room_id: String) -> void:
	_status_label.text = "Joining %s..." % room_id
	NetworkManager.send_join_room(room_id)

# ── Network callbacks ────────────────────────────────────────────────────────

func _on_welcome(_player_id: int) -> void:
	_status_label.text = "Connected — choose a territory"

func _on_room_list(rooms: Array[Dictionary]) -> void:
	for r: Dictionary in rooms:
		var rid: String = r["room_id"]
		if rid in _count_labels:
			(_count_labels[rid] as Label).text = "%d players" % r["player_count"]

func _on_room_joined(room_id: String, spawn_x: int, spawn_y: int, players: Array[Dictionary]) -> void:
	# Disconnect lobby signals before switching scenes.
	NetworkManager.welcome_received.disconnect(_on_welcome)
	NetworkManager.room_list_received.disconnect(_on_room_list)
	NetworkManager.room_joined.disconnect(_on_room_joined)
	NetworkManager.disconnected.disconnect(_on_disconnected)
	# Switch to the world scene, passing room context.
	var world := WorldScene.instantiate()
	world.room_id = room_id
	world.room_spawn = Vector2i(spawn_x, spawn_y)
	world.room_players = players
	get_tree().root.add_child(world)
	queue_free()

func _on_disconnected() -> void:
	_status_label.text = "Disconnected — reconnecting..."
	# Try to reconnect after a short delay.
	await get_tree().create_timer(2.0).timeout
	if is_inside_tree():
		NetworkManager.connect_to_server()
