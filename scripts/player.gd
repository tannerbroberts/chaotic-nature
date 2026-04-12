extends Node2D

@export var tilemap_path: NodePath

var tilemap: TileMapLayer
var tile_pos := Vector2i.ZERO
var walk_queue: Array[Vector2i] = []
var is_running := false

var _visual_start := Vector2.ZERO
var _visual_target := Vector2.ZERO
var _lerp_progress := 1.0  # Start fully arrived.

func _ready() -> void:
	tilemap = get_node(tilemap_path) as TileMapLayer
	GameTickManager.tick.connect(_on_tick)
	_visual_start = position
	_visual_target = position
	# Create a placeholder sprite if the Sprite2D has no texture.
	var sprite := $Sprite2D as Sprite2D
	if sprite and sprite.texture == null:
		var img := Image.create(24, 24, false, Image.FORMAT_RGBA8)
		img.fill(Color(0.2, 0.4, 0.9))  # Blue player square.
		sprite.texture = ImageTexture.create_from_image(img)

func set_tile_pos(new_tile: Vector2i) -> void:
	tile_pos = new_tile
	_visual_start = position
	_visual_target = tilemap.map_to_local(tile_pos)
	_lerp_progress = 0.0

func _on_tick() -> void:
	if tilemap == null:
		return
	var steps := 2 if is_running else 1
	for i in steps:
		if walk_queue.is_empty():
			break
		tile_pos = walk_queue.pop_front()
	var new_target := tilemap.map_to_local(tile_pos)
	if new_target != _visual_target:
		_visual_start = position
		_visual_target = new_target
		_lerp_progress = 0.0

func _process(delta: float) -> void:
	if _lerp_progress < 1.0:
		_lerp_progress = minf(_lerp_progress + delta / GameTickManager.TICK_DURATION, 1.0)
		# Smoothstep easing: smooth acceleration and deceleration.
		var t := _lerp_progress * _lerp_progress * (3.0 - 2.0 * _lerp_progress)
		position = _visual_start.lerp(_visual_target, t)
