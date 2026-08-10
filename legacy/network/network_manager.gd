extends Node
## NetworkManager — Autoload singleton that manages the WebSocket connection
## to the authoritative game server.
##
## Emits signals that the rest of the game listens to. Sends player inputs
## (MOVE_TO, TOGGLE_RUN) and receives authoritative state (TICK_STATE, etc.).

const CodecScript = preload("res://scripts/network/codec.gd")

signal connected
signal disconnected
signal welcome_received(player_id: int)
signal tick_state_received(tick_number: int, players: Array[Dictionary])
signal player_joined(player_id: int, tile_x: int, tile_y: int)
signal player_left(player_id: int)
signal room_list_received(rooms: Array[Dictionary])
signal room_joined(room_id: String, spawn_x: int, spawn_y: int, players: Array[Dictionary])
signal room_transfer_started(dest_room_id: String, transfer_time_ms: int)

const SERVER_URL := "ws://127.0.0.1:9001"

var my_player_id := -1

var _socket := WebSocketPeer.new()
var _connected := false

func _ready() -> void:
	set_process(false)

func is_connected_to_server() -> bool:
	return _connected

## Call this to initiate the connection. Can be called from a menu or on startup.
func connect_to_server() -> void:
	var err := _socket.connect_to_url(SERVER_URL)
	if err != OK:
		push_error("NetworkManager: failed to connect — ", err)
		return
	set_process(true)

func _process(_delta: float) -> void:
	_socket.poll()

	var state := _socket.get_ready_state()

	if state == WebSocketPeer.STATE_OPEN:
		if not _connected:
			_connected = true
			connected.emit()
			print("NetworkManager: connected to ", SERVER_URL)
		# Drain incoming messages.
		while _socket.get_available_packet_count() > 0:
			var data := _socket.get_packet()
			_handle_packet(data)

	elif state == WebSocketPeer.STATE_CLOSED:
		if _connected:
			_connected = false
			my_player_id = -1
			disconnected.emit()
			print("NetworkManager: disconnected (code ", _socket.get_close_code(), ")")
		set_process(false)

# ── Send helpers ─────────────────────────────────────────────────────────────

func send_move_to(target_x: int, target_y: int) -> void:
	if not _connected:
		return
	_socket.send(CodecScript.encode_move_to(target_x, target_y))

func send_toggle_run(running: bool) -> void:
	if not _connected:
		return
	_socket.send(CodecScript.encode_toggle_run(running))

func send_join_room(room_id: String) -> void:
	if not _connected:
		return
	_socket.send(CodecScript.encode_join_room(room_id))

func send_leave_room() -> void:
	if not _connected:
		return
	_socket.send(CodecScript.encode_leave_room())

func send_transfer_request(direction: String) -> void:
	if not _connected:
		return
	_socket.send(CodecScript.encode_transfer_request(direction))

# ── Receive handling ─────────────────────────────────────────────────────────

func _handle_packet(data: PackedByteArray) -> void:
	var msg := CodecScript.decode(data)
	match msg.type:
		CodecScript.Msg.WELCOME:
			my_player_id = msg.payload["player_id"]
			welcome_received.emit(
				msg.payload["player_id"],
			)
		CodecScript.Msg.TICK_STATE:
			tick_state_received.emit(
				msg.payload["tick_number"],
				msg.payload["players"],
			)
		CodecScript.Msg.PLAYER_JOIN:
			player_joined.emit(
				msg.payload["player_id"],
				msg.payload["tile_x"],
				msg.payload["tile_y"],
			)
		CodecScript.Msg.PLAYER_LEAVE:
			player_left.emit(
				msg.payload["player_id"],
			)
		CodecScript.Msg.ROOM_LIST:
			room_list_received.emit(
				msg.payload["rooms"],
			)
		CodecScript.Msg.ROOM_JOINED:
			room_joined.emit(
				msg.payload["room_id"],
				msg.payload["spawn_x"],
				msg.payload["spawn_y"],
				msg.payload["players"],
			)
		CodecScript.Msg.ROOM_TRANSFER:
			room_transfer_started.emit(
				msg.payload["dest_room_id"],
				msg.payload["transfer_time_ms"],
			)
