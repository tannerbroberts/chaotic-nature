extends Camera2D

@export var target_path: NodePath
@export var smooth_speed := 4.0

var target: Node2D

func _ready() -> void:
	target = get_node(target_path) as Node2D

func _process(delta: float) -> void:
	if target:
		global_position = global_position.lerp(target.global_position, smooth_speed * delta)
