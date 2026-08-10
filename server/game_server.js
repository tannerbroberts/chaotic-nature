/**
 * game_server.js — Authoritative WebSocket game server for Chaotic Nature.
 *
 * Manages a 2×2 grid of Room instances, a lobby state for new connections,
 * and a single 0.6 s tick loop that drives all rooms.
 *
 * Usage:
 *   node game_server.js [port]            (default 9001)
 */

'use strict';

const { WebSocketServer } = require('ws');
const codec = require('./codec');
const Room = require('./room');

// ── Config ───────────────────────────────────────────────────────────────────

const PORT = parseInt(process.argv[2] || '9001', 10);
const TICK_DURATION_MS = 600; // 0.6 s
const MAX_PLAYERS = 60;

// ── Global state ─────────────────────────────────────────────────────────────

let nextPlayerId = 1;
let tickCount = 0;

/**
 * Per-connection state (lives outside any room).
 * @typedef {Object} Connection
 * @property {number} id
 * @property {import('ws').WebSocket} ws
 * @property {string|null} roomId       — null = in lobby
 * @property {string|null} transferTo   — room being transferred to (in queue)
 */

/** @type {Map<import('ws').WebSocket, Connection>} */
const connections = new Map();

// ── Room grid (2×2) ─────────────────────────────────────────────────────────

/** @type {Map<string, Room>} */
const rooms = new Map();

function createRooms() {
  const coords = [
    [0, 0], [1, 0],
    [0, 1], [1, 1],
  ];
  for (const [gx, gy] of coords) {
    const id = `${gx},${gy}`;
    rooms.set(id, new Room(id, gx, gy));
  }

  // Wire up exit tiles. 3 turnstile tiles per edge, each mapping 1:1 to the
  // corresponding tile on the adjacent room's edge.
  const exitDefs = [
    // [room, direction, destRoom, [[exitTile, entryTile], ...]]
    ['0,0', 'east',  '1,0', [[{x:15,y:7},{x:0,y:7}],[{x:15,y:8},{x:0,y:8}],[{x:15,y:9},{x:0,y:9}]]],
    ['0,0', 'south', '0,1', [[{x:7,y:15},{x:7,y:0}],[{x:8,y:15},{x:8,y:0}],[{x:9,y:15},{x:9,y:0}]]],
    ['1,0', 'west',  '0,0', [[{x:0,y:7},{x:15,y:7}],[{x:0,y:8},{x:15,y:8}],[{x:0,y:9},{x:15,y:9}]]],
    ['1,0', 'south', '1,1', [[{x:7,y:15},{x:7,y:0}],[{x:8,y:15},{x:8,y:0}],[{x:9,y:15},{x:9,y:0}]]],
    ['0,1', 'east',  '1,1', [[{x:15,y:7},{x:0,y:7}],[{x:15,y:8},{x:0,y:8}],[{x:15,y:9},{x:0,y:9}]]],
    ['0,1', 'north', '0,0', [[{x:7,y:0},{x:7,y:15}],[{x:8,y:0},{x:8,y:15}],[{x:9,y:0},{x:9,y:15}]]],
    ['1,1', 'west',  '0,1', [[{x:0,y:7},{x:15,y:7}],[{x:0,y:8},{x:15,y:8}],[{x:0,y:9},{x:15,y:9}]]],
    ['1,1', 'north', '1,0', [[{x:7,y:0},{x:7,y:15}],[{x:8,y:0},{x:8,y:15}],[{x:9,y:0},{x:9,y:15}]]],
  ];

  for (const [roomId, dir, destId, pairs] of exitDefs) {
    const tilePairs = pairs.map(([exit, entry]) => ({ exit, entry }));
    rooms.get(roomId).exits.set(dir, { destRoomId: destId, tilePairs });
  }

  console.log(`Created ${rooms.size} rooms in a 2×2 grid`);
  for (const [id, room] of rooms) {
    const exitDirs = [...room.exits.keys()].join(', ');
    console.log(`  Room ${id}: exits [${exitDirs}]`);
  }
}

createRooms();

// ── Room list helper ─────────────────────────────────────────────────────────

function buildRoomListMsg() {
  const list = [];
  for (const [, room] of rooms) {
    list.push({
      id: room.id,
      playerCount: room.playerCount,
      gridX: room.gridX,
      gridY: room.gridY,
    });
  }
  return codec.encodeRoomList(list);
}

/** Send the current room list to all lobby players. */
function broadcastRoomListToLobby() {
  const msg = buildRoomListMsg();
  for (const [, conn] of connections) {
    if (conn.roomId === null && conn.transferTo === null && conn.ws.readyState === conn.ws.OPEN) {
      conn.ws.send(msg);
    }
  }
}

// ── WebSocket server ─────────────────────────────────────────────────────────

const wss = new WebSocketServer({ port: PORT });

wss.on('listening', () => {
  console.log(`Game server listening on ws://0.0.0.0:${PORT}`);
});

wss.on('connection', (ws) => {
  const conn = {
    id: nextPlayerId++,
    ws,
    roomId: null,
    transferTo: null,
  };
  connections.set(ws, conn);

  // Send WELCOME (player id only — they're in the lobby).
  ws.send(codec.encodeWelcome(conn.id));

  // Send current room list.
  ws.send(buildRoomListMsg());

  console.log(`Player ${conn.id} connected → lobby (${connections.size} total)`);

  ws.on('message', (data) => {
    handleMessage(conn, data);
  });

  ws.on('close', () => {
    // Clean up: remove from room or transfer queue.
    if (conn.roomId) {
      const room = rooms.get(conn.roomId);
      if (room) {
        room.removePlayer(conn.id);
        console.log(`Player ${conn.id} left room ${conn.roomId}`);
      }
    }
    if (conn.transferTo) {
      const destRoom = rooms.get(conn.transferTo);
      if (destRoom) destRoom.cancelTransfer(conn.id);
    }
    connections.delete(ws);
    broadcastRoomListToLobby();
    console.log(`Player ${conn.id} disconnected (${connections.size} total)`);
  });

  ws.on('error', (err) => {
    console.error(`Player ${conn.id} error:`, err.message);
  });
});

// ── Message handling ─────────────────────────────────────────────────────────

function handleMessage(conn, data) {
  let msg;
  try {
    msg = codec.decode(data);
  } catch {
    return;
  }

  switch (msg.type) {
    case 3: { // JOIN_ROOM
      const { roomId } = msg.payload;
      if (conn.roomId !== null || conn.transferTo !== null) break; // Already in a room or transferring.
      const room = rooms.get(roomId);
      if (!room) break;
      if (room.playerCount >= MAX_PLAYERS) break;
      conn.roomId = roomId;
      room.addPlayer(conn.id, conn.ws, Room.SPAWN_X, Room.SPAWN_Y, false);
      broadcastRoomListToLobby();
      console.log(`Player ${conn.id} joined room ${roomId} (${room.playerCount}/${MAX_PLAYERS})`);
      break;
    }
    case 4: { // LEAVE_ROOM
      if (conn.roomId === null) break;
      const room = rooms.get(conn.roomId);
      if (room) {
        room.removePlayer(conn.id);
        console.log(`Player ${conn.id} left room ${conn.roomId}`);
      }
      conn.roomId = null;
      conn.transferTo = null;
      // Send them back to lobby with fresh room list.
      conn.ws.send(buildRoomListMsg());
      broadcastRoomListToLobby();
      break;
    }
    case 1: // MOVE_TO
    case 2: { // TOGGLE_RUN
      // Forward to the player's current room.
      if (conn.roomId) {
        const room = rooms.get(conn.roomId);
        if (room) room.handleMessage(conn.id, msg);
      }
      break;
    }
    case 5: { // TRANSFER_REQUEST
      if (!conn.roomId || conn.transferTo) break;
      const room = rooms.get(conn.roomId);
      if (!room) break;
      const direction = msg.payload.direction;
      const exit = room.exits.get(direction);
      if (!exit) break;
      const player = room.players.get(conn.id);
      if (!player) break;
      // Find the tile pair matching the player's current position.
      const pair = exit.tilePairs.find(p => p.exit.x === player.tileX && p.exit.y === player.tileY);
      if (!pair) break; // Player isn't on a valid exit tile for this direction.
      const destRoom = rooms.get(exit.destRoomId);
      if (!destRoom || destRoom.playerCount >= MAX_PLAYERS) break;
      // Remove from current room and initiate transfer.
      room.removePlayer(conn.id);
      conn.roomId = null;
      conn.transferTo = exit.destRoomId;
      if (conn.ws.readyState === conn.ws.OPEN) {
        conn.ws.send(codec.encodeRoomTransfer(exit.destRoomId, Room.TRANSFER_DELAY_MS));
      }
      destRoom.queueTransfer(conn.id, conn.ws, pair.entry, player.running);
      console.log(`Player ${conn.id} transfer requested: ${direction} (${pair.exit.x},${pair.exit.y}) → ${exit.destRoomId} (${pair.entry.x},${pair.entry.y})`);
      broadcastRoomListToLobby();
      break;
    }
  }
}

// ── Tick loop ────────────────────────────────────────────────────────────────

function tick() {
  tickCount++;

  for (const [, room] of rooms) {
    room.tick(tickCount);
  }

  // Check if any transfers just completed this tick (players were admitted).
  // Update their connection state.
  for (const [, room] of rooms) {
    for (const [playerId] of room.players) {
      const conn = findConnection(playerId);
      if (conn && conn.transferTo === room.id) {
        conn.roomId = room.id;
        conn.transferTo = null;
        broadcastRoomListToLobby();
      }
    }
  }
}

setInterval(tick, TICK_DURATION_MS);

// ── Helpers ──────────────────────────────────────────────────────────────────

function findConnection(playerId) {
  for (const [, conn] of connections) {
    if (conn.id === playerId) return conn;
  }
  return null;
}

console.log(`Tick duration: ${TICK_DURATION_MS}ms, Max players per room: ${MAX_PLAYERS}`);
