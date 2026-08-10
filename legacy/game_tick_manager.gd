extends Node

signal tick

const TICK_DURATION := 0.6

var tick_count := 0
var _accumulator := 0.0

func _process(delta: float) -> void:
	_accumulator += delta
	while _accumulator >= TICK_DURATION:
		_accumulator -= TICK_DURATION
		tick_count += 1
		tick.emit()
