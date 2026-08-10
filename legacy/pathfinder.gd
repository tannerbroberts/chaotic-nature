class_name Pathfinder
extends RefCounted

## Collision flag bit layout — matches the custom data on TileMapLayer.
enum Dir {
	N  = 1,
	NE = 2,
	E  = 4,
	SE = 8,
	S  = 16,
	SW = 32,
	W  = 64,
	NW = 128,
}

## Maps a direction offset to (required_exit_flag, required_entry_flag).
## To move from tile A in direction D to tile B:
##   A must NOT have exit flag for D, B must NOT have entry flag for opposite(D).
const DIRECTION_DATA: Array[Dictionary] = [
	{"offset": Vector2i( 0, -1), "exit": Dir.N,  "entry": Dir.S},   # North
	{"offset": Vector2i( 1, -1), "exit": Dir.NE, "entry": Dir.SW},  # NE
	{"offset": Vector2i( 1,  0), "exit": Dir.E,  "entry": Dir.W},   # East
	{"offset": Vector2i( 1,  1), "exit": Dir.SE, "entry": Dir.NW},  # SE
	{"offset": Vector2i( 0,  1), "exit": Dir.S,  "entry": Dir.N},   # South
	{"offset": Vector2i(-1,  1), "exit": Dir.SW, "entry": Dir.NE},  # SW
	{"offset": Vector2i(-1,  0), "exit": Dir.W,  "entry": Dir.E},   # West
	{"offset": Vector2i(-1, -1), "exit": Dir.NW, "entry": Dir.SE},  # NW
]

var _tilemap: TileMapLayer

func _init(tilemap: TileMapLayer) -> void:
	_tilemap = tilemap

## Returns collision flags for a tile. 0 means fully walkable.
func _get_flags(tile: Vector2i) -> int:
	var data := _tilemap.get_cell_tile_data(tile)
	if data == null:
		return 255  # Treat empty/missing tiles as fully blocked.
	return data.get_custom_data("collision_flags") as int

## Returns true if a tile coordinate is within the used rect of the tilemap.
func _in_bounds(tile: Vector2i) -> bool:
	var used := _tilemap.get_used_rect()
	return used.has_point(tile)

## Check if a single step from one tile to an adjacent tile is valid.
func _can_step(from: Vector2i, to: Vector2i) -> bool:
	var offset := to - from
	for dir_data: Dictionary in DIRECTION_DATA:
		if (dir_data["offset"] as Vector2i) == offset:
			var from_flags := _get_flags(from)
			if from_flags & (dir_data["exit"] as int):
				return false
			var to_flags := _get_flags(to)
			if to_flags & (dir_data["entry"] as int):
				return false
			if offset.x != 0 and offset.y != 0:
				var h_tile := Vector2i(from.x + offset.x, from.y)
				var v_tile := Vector2i(from.x, from.y + offset.y)
				if _get_flags(h_tile) == 255 or _get_flags(v_tile) == 255:
					return false
			return true
	return false

## Build a direct path preferring the larger cardinal axis.
## Move horizontal when |dx| > |dy|, vertical when |dy| > |dx|, diagonal when equal.
## Returns empty array if any step is blocked (caller should fall back to BFS).
func _find_direct_path(start: Vector2i, end: Vector2i) -> Array[Vector2i]:
	var path: Array[Vector2i] = []
	var current := start
	while current != end:
		var dx := end.x - current.x
		var dy := end.y - current.y
		var abs_dx := absi(dx)
		var abs_dy := absi(dy)
		var step: Vector2i
		if abs_dx > abs_dy:
			step = Vector2i(signi(dx), 0)
		elif abs_dy > abs_dx:
			step = Vector2i(0, signi(dy))
		else:
			step = Vector2i(signi(dx), signi(dy))
		var next := current + step
		if not _in_bounds(next) or not _can_step(current, next):
			return [] as Array[Vector2i]
		current = next
		path.push_back(current)
	return path

## Returns Array[Vector2i] from start (exclusive) to end (inclusive).
## Tries a direct axis-preference path first, falls back to BFS if blocked.
func find_path(start: Vector2i, end: Vector2i) -> Array[Vector2i]:
	if start == end:
		return [] as Array[Vector2i]
	if not _in_bounds(end):
		return [] as Array[Vector2i]
	var direct := _find_direct_path(start, end)
	if direct.size() > 0:
		return direct
	return _find_path_bfs(start, end)

## BFS pathfinding fallback.
func _find_path_bfs(start: Vector2i, end: Vector2i) -> Array[Vector2i]:
	var queue: Array[Vector2i] = [start]
	var came_from: Dictionary = {start: start}

	while queue.size() > 0:
		var current := queue.pop_front() as Vector2i
		if current == end:
			break

		var current_flags := _get_flags(current)

		for dir_data: Dictionary in DIRECTION_DATA:
			var neighbor: Vector2i = current + (dir_data["offset"] as Vector2i)

			if neighbor in came_from:
				continue
			if not _in_bounds(neighbor):
				continue

			if current_flags & (dir_data["exit"] as int):
				continue

			var neighbor_flags := _get_flags(neighbor)
			if neighbor_flags & (dir_data["entry"] as int):
				continue

			var offset: Vector2i = dir_data["offset"] as Vector2i
			if offset.x != 0 and offset.y != 0:
				var h_tile := Vector2i(current.x + offset.x, current.y)
				var v_tile := Vector2i(current.x, current.y + offset.y)
				if _get_flags(h_tile) == 255 or _get_flags(v_tile) == 255:
					continue

			came_from[neighbor] = current
			queue.push_back(neighbor)

	if end not in came_from:
		return [] as Array[Vector2i]

	var path: Array[Vector2i] = []
	var trace := end
	while trace != start:
		path.push_back(trace)
		trace = came_from[trace] as Vector2i
	path.reverse()
	return path
