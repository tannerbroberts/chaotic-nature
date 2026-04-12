/**
 * game_server.js — Authoritative WebSocket game server for Chaotic Nature.
 *
 * Runs the 0.6 s tick loop, owns all player state and pathfinding.
 * Clients send inputs (MOVE_TO, TOGGLE_RUN) and receive authoritative
 * state (TICK_STATE, PLAYER_JOIN, PLAYER_LEAVE, WELCOME).
 *
 * Usage:
 *   node game_server.js [port]            (default 9001)
 */

'use strict';

const { WebSocketServer } = require('ws');
const Pathfinder = require('./pathfinder');
const codec = require('./codec');

// ── Config ───────────────────────────────────────────────────────────────────

const PORT = parseInt(process.argv[2] || '9001', 10);
const TICK_DURATION_MS = 600; // 0.6 s
const GRID_WIDTH = 16;
const GRID_HEIGHT = 16;
const SPAWN_X = 5;
const SPAWN_Y = 5;
const MAX_PLAYERS = 60;

// ── World state ──────────────────────────────────────────────────────────────

// Flat collision grid — 0 = fully walkable (matches the Godot TileMapLayer setup).
const collisionFlags = new Int32Array(GRID_WIDTH * GRID_HEIGHT); // all zeros
const pathfinder = new Pathfinder(GRID_WIDTH, GRID_HEIGHT, collisionFlags);

let nextPlayerId = 1;
let tickCount = 0;

/**
 * @typedef {Object} PlayerState
 * @property {number} id
 * @property {number} tileX
 * @property {number} tileY
 * @property {boolean} running
 * @property {{ x: number, y: number }[]} walkQueue
 * @property {import('ws').WebSocket} ws
 */

/** @type {Map<import('ws').WebSocket, PlayerState>} */
const players = new Map();

// ── WebSocket server ─────────────────────────────────────────────────────────

const wss = new WebSocketServer({ port: PORT });

wss.on('listening', () => {
  console.log(`Game server listening on ws://0.0.0.0:${PORT}`);
});

wss.on('connection', (ws) => {
  if (players.size >= MAX_PLAYERS) {
    ws.close(1013, 'Instance full');
    return;
  }

  const player = {
    id: nextPlayerId++,
    tileX: SPAWN_X,
    tileY: SPAWN_Y,
    running: false,
    walkQueue: [],
    ws,
  };
  players.set(ws, player);

  // Tell the new player their id and spawn position.
  ws.send(codec.encodeWelcome(player.id, player.tileX, player.tileY));

  // Tell the new player about all existing players.
  for (const [, other] of players) {
    if (other === player) continue;
    ws.send(codec.encodePlayerJoin(other.id, other.tileX, other.tileY));
  }

  // Tell existing players about the new player.
  broadcastExcept(ws, codec.encodePlayerJoin(player.id, player.tileX, player.tileY));

  console.log(`Player ${player.id} joined (${players.size}/${MAX_PLAYERS})`);

  ws.on('message', (data) => {
    handleMessage(player, data);
  });

  ws.on('close', () => {
    players.delete(ws);
    broadcast(codec.encodePlayerLeave(player.id));
    console.log(`Player ${player.id} left (${players.size}/${MAX_PLAYERS})`);
  });

  ws.on('error', (err) => {
    console.error(`Player ${player.id} error:`, err.message);
  });
});

// ── Message handling ─────────────────────────────────────────────────────────

/**
 * @param {PlayerState} player
 * @param {Buffer} data
 */
function handleMessage(player, data) {
  let msg;
  try {
    msg = codec.decode(data);
  } catch {
    return; // Malformed — ignore.
  }

  switch (msg.type) {
    case 1: { // MOVE_TO
      const { targetX, targetY } = msg.payload;
      const path = pathfinder.findPath(player.tileX, player.tileY, targetX, targetY);
      player.walkQueue = path; // Replace queue (OSRS behaviour).
      break;
    }
    case 2: { // TOGGLE_RUN
      player.running = msg.payload.running;
      break;
    }
    default:
      // Unknown client message — ignore (forward compat).
      break;
  }
}

// ── Tick loop ────────────────────────────────────────────────────────────────

function tick() {
  tickCount++;

  // Process movement for every player.
  for (const [, player] of players) {
    const steps = player.running ? 2 : 1;
    for (let i = 0; i < steps; i++) {
      if (player.walkQueue.length === 0) break;
      const next = player.walkQueue.shift();
      player.tileX = next.x;
      player.tileY = next.y;
    }
  }

  // Build and broadcast TICK_STATE.
  const snapshot = [];
  for (const [, player] of players) {
    snapshot.push({
      id: player.id,
      tileX: player.tileX,
      tileY: player.tileY,
      running: player.running,
    });
  }
  const tickMsg = codec.encodeTickState(tickCount, snapshot);
  broadcast(tickMsg);
}

setInterval(tick, TICK_DURATION_MS);

// ── Helpers ──────────────────────────────────────────────────────────────────

/** Send a binary message to all connected players. */
function broadcast(data) {
  for (const [ws] of players) {
    if (ws.readyState === ws.OPEN) ws.send(data);
  }
}

/** Send a binary message to all connected players except `exclude`. */
function broadcastExcept(exclude, data) {
  for (const [ws] of players) {
    if (ws !== exclude && ws.readyState === ws.OPEN) ws.send(data);
  }
}
