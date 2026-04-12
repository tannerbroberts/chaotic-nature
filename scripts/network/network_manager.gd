extends Node
## NetworkManager — Autoload singleton that manages the WebSocket connection
## to the authoritative game server.
##
## Emits signals that the rest of the game listens to. Sends player inputs
## (MOVE_TO, TOGGLE_RUN) and receives authoritative state (TICK_STATE, etc.).

const CodecScript = preload("res://scripts/network/codec.gd")

signal connected
signal disconnected
signal welcome_received(player_id: int, tile_x: int, tile_y: int)
signal tick_state_received(tick_number: int, players: Array[Dictionary])
signal player_joined(player_id: int, tile_x: int, tile_y: int)
signal player_left(player_id: int)

const WS_PORT := 9001

var server_url := ""
var my_player_id := -1

var _socket := WebSocketPeer.new()
var _connected := false

func _ready() -> void:
	set_process(false)

## Build the WebSocket URL from the browser's current hostname so the client
## always connects back to the same machine that served the page — works on
## localhost, LAN IPs, and future production domains with zero config.
func _resolve_server_url() -> String:
	var host := "127.0.0.1"
	if OS.has_feature("web"):
		var js_host: String = JavaScriptBridge.eval("window.location.hostname", true)
		if js_host != "":
			host = js_host
	return "ws://%s:%d" % [host, WS_PORT]

## Call this to initiate the connection. Can be called from a menu or on startup.
func connect_to_server(url := "") -> void:
	if url != "":
		server_url = url
	elif server_url == "":
		server_url = _resolve_server_url()
	var err := _socket.connect_to_url(server_url)
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
			print("NetworkManager: connected to ", server_url)
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

# ── Receive handling ─────────────────────────────────────────────────────────

func _handle_packet(data: PackedByteArray) -> void:
	var msg := CodecScript.decode(data)
	match msg.type:
		CodecScript.Msg.WELCOME:
			my_player_id = msg.payload["player_id"]
			welcome_received.emit(
				msg.payload["player_id"],
				msg.payload["tile_x"],
				msg.payload["tile_y"],
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
