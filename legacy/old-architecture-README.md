# Chaotic Nature

An OSRS-style tile-based multiplayer game built with Godot 4.6 (web export) and a Node.js authoritative server.

## Architecture

```
Godot Client (browser)         Node.js Server
──────────────────────         ──────────────
tap tile → send MOVE_TO ──WS──→ pathfind + queue
                                tick (0.6 s)
receive TICK_STATE ←──WS────── broadcast positions
interpolate visually            of all players
```

The server owns the tick clock, pathfinding, and all player state. Clients send inputs and render the authoritative state.

## Network Protocol

All messages use a binary **TLV** (Type-Length-Value) wire format:

```
[ type: u8 | length: u16 | payload: N bytes ]
```

The schema is defined in [`server/protocol.json`](server/protocol.json) — this is the single source of truth for every message in the game. Both the JS server codec (`server/codec.js`) and the GDScript client codec (`scripts/network/codec.gd`) implement this schema.

Message ID ranges are reserved by system:
- **1–9** — Movement
- **10–19** — State sync
- **20–29** — Combat (future)
- **30–39** — Skills (future)

Unknown message types are safely skipped via the length field (forward compatibility).

## Running

### Game server

```bash
cd server
npm install
node game_server.js        # default port 9001
```

### Web export (dev)

```bash
cd exports
node serve.js 8080         # or: python3 serve.py 8080
```

Then open `https://localhost:8080` in a browser.

## Project Structure

```
server/
  protocol.json       ← wire protocol schema (source of truth)
  codec.js            ← JS binary encoder/decoder
  pathfinder.js       ← BFS tile pathfinder (port of GDScript version)
  game_server.js      ← tick loop, WebSocket, player state

scripts/
  network/
    codec.gd          ← GDScript binary encoder/decoder
    network_manager.gd ← autoload — WS connection + signal dispatch
  player.gd           ← movement, visual interpolation
  pathfinder.gd       ← client-side pathfinder (for local prediction, future)
  game_tick_manager.gd ← 0.6 s tick autoload
  ...

scenes/               ← Godot scene files
exports/              ← web export build + dev servers
docs/                 ← architecture docs
```
