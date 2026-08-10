/**
 * room.js — A single room instance within the 2×2 territory grid.
 *
 * Each room owns its players, collision grid, pathfinder, exit tiles,
 * and a transfer queue for incoming players.  The main game_server.js
 * drives all rooms from a single setInterval tick loop.
 */

'use strict';

const Pathfinder = require('./pathfinder');
const codec = require('./codec');

const GRID_WIDTH = 16;
const GRID_HEIGHT = 16;
const SPAWN_X = 5;
const SPAWN_Y = 5;
const TRANSFER_DELAY_MS = 3500;

class Room {
  /**
   * @param {string} id       e.g. "0,0"
   * @param {number} gridX    column in the 2×2 meta-grid
   * @param {number} gridY    row in the 2×2 meta-grid
   */
  constructor(id, gridX, gridY) {
    this.id = id;
    this.gridX = gridX;
    this.gridY = gridY;

    /** @type {Map<number, PlayerState>}  playerId → PlayerState */
    this.players = new Map();

    // Flat collision grid — 0 = fully walkable.
    this.collisionFlags = new Int32Array(GRID_WIDTH * GRID_HEIGHT);
    this.pathfinder = new Pathfinder(GRID_WIDTH, GRID_HEIGHT, this.collisionFlags);

    /**
     * Exit tiles: direction → { destRoomId, tilePairs: [{exit:{x,y}, entry:{x,y}}] }
     * Populated by game_server after all rooms are created.
     * @type {Map<string, { destRoomId: string, tilePairs: {exit:{x:number,y:number}, entry:{x:number,y:number}}[] }>}
     */
    this.exits = new Map();

    /**
     * Transfer queue — players waiting to materialize in this room.
     * @type {{ playerId: number, ws: import('ws').WebSocket, entryTile: {x:number,y:number}, readyAt: number, running: boolean }[]}
     */
    this.transferQueue = [];
  }

  get playerCount() {
    return this.players.size;
  }

  // ── Player management ───────────────────────────────────────────────────

  /**
   * Add a player to this room at a given tile.
   * Sends ROOM_JOINED to the player and PLAYER_JOIN to everyone else.
   */
  addPlayer(playerId, ws, tileX, tileY, running) {
    const player = {
      id: playerId,
      tileX,
      tileY,
      running: running || false,
      walkQueue: [],
      ws,
    };
    this.players.set(playerId, player);

    // Build list of existing players for the joiner.
    const existing = [];
    for (const [, p] of this.players) {
      existing.push({ id: p.id, tileX: p.tileX, tileY: p.tileY });
    }

    // Send ROOM_JOINED to the new player.
    if (ws.readyState === ws.OPEN) {
      ws.send(codec.encodeRoomJoined(this.id, tileX, tileY, existing));
    }

    // Broadcast PLAYER_JOIN to others.
    this.broadcastExcept(playerId, codec.encodePlayerJoin(playerId, tileX, tileY));
  }

  /**
   * Remove a player from this room.
   * Broadcasts PLAYER_LEAVE to remaining players.
   * @returns {PlayerState|null} the removed player state, or null
   */
  removePlayer(playerId) {
    const player = this.players.get(playerId);
    if (!player) return null;
    this.players.delete(playerId);
    this.broadcast(codec.encodePlayerLeave(playerId));
    return player;
  }

  // ── Message handling ────────────────────────────────────────────────────

  handleMessage(playerId, msg) {
    const player = this.players.get(playerId);
    if (!player) return;

    switch (msg.type) {
      case 1: { // MOVE_TO
        const { targetX, targetY } = msg.payload;
        const path = this.pathfinder.findPath(player.tileX, player.tileY, targetX, targetY);
        player.walkQueue = path;
        break;
      }
      case 2: { // TOGGLE_RUN
        player.running = msg.payload.running;
        break;
      }
    }
  }

  // ── Tick ────────────────────────────────────────────────────────────────

  /**
   * Advance one tick: move players, admit transfers, broadcast state.
   * @param {number} tickCount
   */
  tick(tickCount) {
    // 1. Process movement.
    for (const [, player] of this.players) {
      const steps = player.running ? 2 : 1;
      for (let i = 0; i < steps; i++) {
        if (player.walkQueue.length === 0) break;
        const next = player.walkQueue.shift();
        player.tileX = next.x;
        player.tileY = next.y;
      }
    }

    // 2. Admit players from the transfer queue.
    const now = Date.now();
    const admitted = [];
    this.transferQueue = this.transferQueue.filter((entry) => {
      if (now >= entry.readyAt) {
        admitted.push(entry);
        return false;
      }
      return true;
    });
    for (const entry of admitted) {
      this.addPlayer(entry.playerId, entry.ws, entry.entryTile.x, entry.entryTile.y, entry.running);
    }

    // 4. Build and broadcast TICK_STATE to this room's players.
    const snapshot = [];
    for (const [, player] of this.players) {
      snapshot.push({
        id: player.id,
        tileX: player.tileX,
        tileY: player.tileY,
        running: player.running,
      });
    }
    const tickMsg = codec.encodeTickState(tickCount, snapshot);
    this.broadcast(tickMsg);
  }

  // ── Transfer queue ──────────────────────────────────────────────────────

  /**
   * Queue a player to materialize in this room after a delay.
   */
  queueTransfer(playerId, ws, entryTile, running) {
    this.transferQueue.push({
      playerId,
      ws,
      entryTile,
      running: running || false,
      readyAt: Date.now() + TRANSFER_DELAY_MS,
    });
  }

  /**
   * Remove a player from the transfer queue (e.g. on disconnect).
   */
  cancelTransfer(playerId) {
    this.transferQueue = this.transferQueue.filter((e) => e.playerId !== playerId);
  }

  // ── Broadcasting ────────────────────────────────────────────────────────

  broadcast(data) {
    for (const [, player] of this.players) {
      if (player.ws.readyState === player.ws.OPEN) {
        player.ws.send(data);
      }
    }
  }

  broadcastExcept(excludeId, data) {
    for (const [, player] of this.players) {
      if (player.id !== excludeId && player.ws.readyState === player.ws.OPEN) {
        player.ws.send(data);
      }
    }
  }
}

module.exports = Room;
module.exports.GRID_WIDTH = GRID_WIDTH;
module.exports.GRID_HEIGHT = GRID_HEIGHT;
module.exports.SPAWN_X = SPAWN_X;
module.exports.SPAWN_Y = SPAWN_Y;
module.exports.TRANSFER_DELAY_MS = TRANSFER_DELAY_MS;
