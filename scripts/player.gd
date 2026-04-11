extends Node2D

@export var tilemap_path: NodePath
@export var interpolation_speed := 10.0

var tilemap: TileMapLayer
var tile_pos := Vector2i.ZERO
var walk_queue: Array[Vector2i] = []
var is_running := false

var _visual_target := Vector2.ZERO

func _ready() -> void:
	tilemap = get_node(tilemap_path) as TileMapLayer
	GameTickManager.tick.connect(_on_tick)
	_visual_target = position
	# Create a placeholder sprite if the Sprite2D has no texture.
	var sprite := $Sprite2D as Sprite2D
	if sprite and sprite.texture == null:
		var img := Image.create(24, 24, false, Image.FORMAT_RGBA8)
		img.fill(Color(0.2, 0.4, 0.9))  # Blue player square.
		sprite.texture = ImageTexture.create_from_image(img)

func set_tile_pos(new_tile: Vector2i) -> void:
	tile_pos = new_tile
	_visual_target = tilemap.map_to_local(tile_pos)

func _on_tick() -> void:
	if tilemap == null:
		return
	var steps := 2 if is_running else 1
	for i in steps:
		if walk_queue.is_empty():
			break
		tile_pos = walk_queue.pop_front()
	_visual_target = tilemap.map_to_local(tile_pos)

func _process(delta: float) -> void:
	position = position.lerp(_visual_target, interpolation_speed * delta)
