extends Node2D

## Hub-and-spokes radial action menu displayed at POI tiles.
## 1 central button + 6 surrounding buttons arranged hexagonally.
## Each button is a circle with diameter ≈ tile size (64 px).
## Spoke gap = half-radius (16 px) between button edges.
## Disabled buttons render gray at 50% opacity and ignore clicks.

const BUTTON_RADIUS := 32.0
const SPOKE_DISTANCE := 80.0  # 32 + 16 + 32 center-to-center

const DISABLED_COLOR := Color(0.45, 0.45, 0.45, 0.5)
const DISABLED_BORDER := Color(1, 1, 1, 0.15)
const ENABLED_BORDER := Color(1, 1, 1, 0.4)

signal button_pressed(index: int)

## Each entry: { "pos": Vector2, "color": Color, "enabled": bool }
var _buttons: Array[Dictionary] = []

func _ready() -> void:
	visible = false
	z_index = 100
	_setup_buttons()

func _setup_buttons() -> void:
	_buttons.clear()
	# Index 0: center hub button — enabled (leave/travel action).
	_buttons.append({"pos": Vector2.ZERO, "color": Color(0.35, 0.45, 0.65, 0.9), "enabled": true})
	# Indices 1-6: spoke buttons — disabled by default (no actions yet).
	for i in 6:
		var angle := deg_to_rad(60.0 * i - 90.0)
		var pos := Vector2(cos(angle), sin(angle)) * SPOKE_DISTANCE
		_buttons.append({"pos": pos, "color": Color(0.45, 0.58, 0.38, 0.9), "enabled": false})

## Enable or disable a specific button by index.
func set_button_enabled(index: int, enabled: bool) -> void:
	if index >= 0 and index < _buttons.size():
		_buttons[index]["enabled"] = enabled
		queue_redraw()

func _draw() -> void:
	for i in _buttons.size():
		var btn: Dictionary = _buttons[i]
		var pos: Vector2 = btn["pos"]
		var enabled: bool = btn["enabled"]
		var fill_color: Color = btn["color"] if enabled else DISABLED_COLOR
		var border_color: Color = ENABLED_BORDER if enabled else DISABLED_BORDER
		draw_circle(pos, BUTTON_RADIUS, fill_color)
		draw_arc(pos, BUTTON_RADIUS, 0, TAU, 32, border_color, 2.0)
	# Draw leave icon on center button (index 0).
	_draw_leave_icon(_buttons[0]["pos"])

func _draw_leave_icon(center: Vector2) -> void:
	# Arrow pointing right with a door-frame bracket on the left.
	var icon_color := Color(1, 1, 1, 0.85)
	var s := BUTTON_RADIUS * 0.45  # Scale factor.
	# Door frame: open bracket shape on the left.
	var door_pts: PackedVector2Array = PackedVector2Array([
		center + Vector2(-0.1 * s, -s),
		center + Vector2(-0.7 * s, -s),
		center + Vector2(-0.7 * s, s),
		center + Vector2(-0.1 * s, s),
	])
	for j in door_pts.size() - 1:
		draw_line(door_pts[j], door_pts[j + 1], icon_color, 2.5)
	# Arrow shaft.
	var shaft_start := center + Vector2(-0.1 * s, 0)
	var shaft_end := center + Vector2(0.8 * s, 0)
	draw_line(shaft_start, shaft_end, icon_color, 2.5)
	# Arrowhead.
	var head_size := 0.35 * s
	draw_line(shaft_end, shaft_end + Vector2(-head_size, -head_size), icon_color, 2.5)
	draw_line(shaft_end, shaft_end + Vector2(-head_size, head_size), icon_color, 2.5)

func _unhandled_input(event: InputEvent) -> void:
	if not visible:
		return
	var screen_pos := Vector2.ZERO
	var is_press := false
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_LEFT and mb.pressed:
			screen_pos = mb.global_position
			is_press = true
	elif event is InputEventScreenTouch:
		var touch := event as InputEventScreenTouch
		if touch.pressed:
			screen_pos = touch.position
			is_press = true
	if not is_press:
		return
	var world_pos := get_viewport().get_canvas_transform().affine_inverse() * screen_pos
	var local_pos := to_local(world_pos)
	for i in _buttons.size():
		if local_pos.distance_to(_buttons[i]["pos"]) <= BUTTON_RADIUS:
			if _buttons[i]["enabled"]:
				button_pressed.emit(i)
			# Consume click on any button circle (enabled or not) to prevent pathfinding.
			get_viewport().set_input_as_handled()
			return

func show_at(world_position: Vector2) -> void:
	position = world_position
	visible = true
	queue_redraw()

func hide_menu() -> void:
	visible = false
