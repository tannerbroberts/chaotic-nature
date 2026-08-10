extends CanvasLayer
## TransitionOverlay — Full-screen fade overlay shown during room transfers.
## Displays "Traveling to Territory (x,y)..." text, fades in, holds, then
## fades out when the destination room sends ROOM_JOINED.

var _color_rect: ColorRect
var _label: Label
var _tween: Tween

func _ready() -> void:
	layer = 100  # Above everything.
	_color_rect = ColorRect.new()
	_color_rect.color = Color(0.0, 0.0, 0.0, 0.0)
	_color_rect.set_anchors_preset(Control.PRESET_FULL_RECT)
	_color_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(_color_rect)

	_label = Label.new()
	_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	_label.set_anchors_preset(Control.PRESET_FULL_RECT)
	_label.add_theme_font_size_override("font_size", 22)
	_label.add_theme_color_override("font_color", Color(0.8, 0.9, 0.7, 0.0))
	_label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(_label)

func show_transfer(dest_room_id: String) -> void:
	_label.text = "Traveling to Territory (%s)..." % dest_room_id
	_color_rect.mouse_filter = Control.MOUSE_FILTER_STOP  # Block input during transfer.
	if _tween:
		_tween.kill()
	_tween = create_tween()
	_tween.set_parallel(true)
	_tween.tween_property(_color_rect, "color:a", 0.85, 0.5)
	_tween.tween_property(_label, "theme_override_colors/font_color:a", 1.0, 0.5)

func hide_transfer() -> void:
	if _tween:
		_tween.kill()
	_tween = create_tween()
	_tween.set_parallel(true)
	_tween.tween_property(_color_rect, "color:a", 0.0, 0.4)
	_tween.tween_property(_label, "theme_override_colors/font_color:a", 0.0, 0.4)
	_tween.chain().tween_callback(func():
		_color_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	)
