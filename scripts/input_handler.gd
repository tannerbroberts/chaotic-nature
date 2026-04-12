extends Node

const PathfinderScript = preload("res://scripts/pathfinder.gd")

@export var tilemap_path: NodePath
@export var player_path: NodePath
@export var tap_highlight_path: NodePath

var tilemap: TileMapLayer
var player: Node2D
var _pathfinder: RefCounted
var _tap_highlight: Node2D

func _ready() -> void:
	tilemap = get_node(tilemap_path) as TileMapLayer
	player = get_node(player_path) as Node2D
	_pathfinder = PathfinderScript.new(tilemap)
	if tap_highlight_path:
		_tap_highlight = get_node(tap_highlight_path)

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_LEFT and mb.pressed:
			_handle_click(mb.global_position)
	elif event is InputEventScreenTouch:
		var touch := event as InputEventScreenTouch
		if touch.pressed:
			_handle_click(touch.position)

func _handle_click(screen_pos: Vector2) -> void:
	# Convert screen position to world position using the canvas transform.
	var world_pos := get_viewport().get_canvas_transform().affine_inverse() * screen_pos
	var clicked_tile := tilemap.local_to_map(tilemap.to_local(world_pos))
	# Show tap-scrim highlight on the top-level asset at this tile.
	if _tap_highlight:
		_tap_highlight.highlight_at(clicked_tile)
	if NetworkManager.my_player_id >= 0:
		# Server-authoritative: send input, server does pathfinding.
		NetworkManager.send_move_to(clicked_tile.x, clicked_tile.y)
	else:
		# Offline fallback: local pathfinding.
		var path: Array[Vector2i] = _pathfinder.find_path(player.tile_pos, clicked_tile)
		if path.size() > 0:
			player.walk_queue = path
