class_name Codec
extends RefCounted
## Binary TLV encoder/decoder for the Chaotic Nature protocol.
##
## Wire format for every message:
##   [ type: u8 | length: u16 | payload: N bytes ]
##
## All multi-byte values are LITTLE-ENDIAN.
## See server/protocol.json for the full schema.

const HEADER_SIZE := 3

# ── Message IDs ──────────────────────────────────────────────────────────────

enum Msg {
	MOVE_TO      = 1,
	TOGGLE_RUN   = 2,
	TICK_STATE   = 10,
	PLAYER_JOIN  = 11,
	PLAYER_LEAVE = 12,
	WELCOME      = 13,
}

# ── Encode (client → server) ────────────────────────────────────────────────

static func encode_move_to(target_x: int, target_y: int) -> PackedByteArray:
	var buf := StreamPeerBuffer.new()
	buf.big_endian = false
	buf.put_u8(Msg.MOVE_TO)
	buf.put_u16(4)  # payload length: i16 + i16
	buf.put_16(target_x)
	buf.put_16(target_y)
	return buf.data_array

static func encode_toggle_run(running: bool) -> PackedByteArray:
	var buf := StreamPeerBuffer.new()
	buf.big_endian = false
	buf.put_u8(Msg.TOGGLE_RUN)
	buf.put_u16(1)
	buf.put_u8(1 if running else 0)
	return buf.data_array

# ── Decode (server → client) ────────────────────────────────────────────────

## Decoded message container.
class Message:
	var type: int
	var payload: Dictionary

	func _init(t: int, p: Dictionary) -> void:
		type = t
		payload = p

static func decode(data: PackedByteArray) -> Message:
	var buf := StreamPeerBuffer.new()
	buf.big_endian = false
	buf.data_array = data

	var type := buf.get_u8()
	var length := buf.get_u16()

	match type:
		Msg.WELCOME:
			var player_id := buf.get_u16()
			var tile_x := buf.get_16()
			var tile_y := buf.get_16()
			return Message.new(type, {
				"player_id": player_id,
				"tile_x": tile_x,
				"tile_y": tile_y,
			})
		Msg.TICK_STATE:
			var tick_number := buf.get_u32()
			var player_count := buf.get_u16()
			var players: Array[Dictionary] = []
			for i in player_count:
				var pid := buf.get_u16()
				var tx := buf.get_16()
				var ty := buf.get_16()
				var flags := buf.get_u8()
				players.append({
					"player_id": pid,
					"tile_x": tx,
					"tile_y": ty,
					"running": (flags & 1) == 1,
				})
			return Message.new(type, {
				"tick_number": tick_number,
				"players": players,
			})
		Msg.PLAYER_JOIN:
			var player_id := buf.get_u16()
			var tile_x := buf.get_16()
			var tile_y := buf.get_16()
			return Message.new(type, {
				"player_id": player_id,
				"tile_x": tile_x,
				"tile_y": tile_y,
			})
		Msg.PLAYER_LEAVE:
			var player_id := buf.get_u16()
			return Message.new(type, {
				"player_id": player_id,
			})
		_:
			# Unknown message — skip payload (forward compat).
			buf.seek(buf.get_position() + length)
			return Message.new(type, {})
