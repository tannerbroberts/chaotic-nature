/**
 * codec.js — Binary TLV encoder/decoder for the Chaotic Nature protocol.
 *
 * Wire format for every message:
 *   ┌──────────┬────────────┬─────────────────┐
 *   │ type (u8)│ length (u16)│ payload (N bytes)│
 *   └──────────┴────────────┴─────────────────┘
 *
 * "type" is the message id from protocol.json.
 * "length" is the byte length of the payload that follows (excludes the 3-byte header).
 *
 * See protocol.json for the full schema.
 */

'use strict';

const HEADER_SIZE = 3; // 1 (type) + 2 (length)

// ── Writers ──────────────────────────────────────────────────────────────────

function writeUint8(view, offset, value) {
  view.setUint8(offset, value);
  return offset + 1;
}

function writeInt16(view, offset, value) {
  view.setInt16(offset, value, true); // little-endian
  return offset + 2;
}

function writeUint16(view, offset, value) {
  view.setUint16(offset, value, true);
  return offset + 2;
}

function writeUint32(view, offset, value) {
  view.setUint32(offset, value, true);
  return offset + 4;
}

function writeInt8(view, offset, value) {
  view.setInt8(offset, value);
  return offset + 1;
}

/** Write a length-prefixed UTF-8 string (u8 length + chars). */
function writeString(view, offset, str) {
  const bytes = Buffer.from(str, 'utf8');
  offset = writeUint8(view, offset, bytes.length);
  for (let i = 0; i < bytes.length; i++) {
    view.setUint8(offset + i, bytes[i]);
  }
  return offset + bytes.length;
}

/** Byte size of a length-prefixed string. */
function stringSize(str) {
  return 1 + Buffer.byteLength(str, 'utf8');
}

// ── Readers ──────────────────────────────────────────────────────────────────

function readUint8(view, offset) {
  return { value: view.getUint8(offset), offset: offset + 1 };
}

function readInt16(view, offset) {
  return { value: view.getInt16(offset, true), offset: offset + 2 };
}

function readUint16(view, offset) {
  return { value: view.getUint16(offset, true), offset: offset + 2 };
}

function readUint32(view, offset) {
  return { value: view.getUint32(offset, true), offset: offset + 4 };
}

function readInt8(view, offset) {
  return { value: view.getInt8(offset), offset: offset + 1 };
}

/** Read a length-prefixed UTF-8 string (u8 length + chars). */
function readString(view, offset) {
  const len = view.getUint8(offset); offset += 1;
  const bytes = new Uint8Array(view.buffer, view.byteOffset + offset, len);
  return { value: Buffer.from(bytes).toString('utf8'), offset: offset + len };
}

// ── Encode ───────────────────────────────────────────────────────────────────

/**
 * Encode a MOVE_TO message (client → server).
 * @param {number} targetX
 * @param {number} targetY
 * @returns {ArrayBuffer}
 */
function encodeMoveTO(targetX, targetY) {
  const payloadSize = 4; // int16 + int16
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 1);              // msg id
  o = writeUint16(v, o, payloadSize);    // length
  o = writeInt16(v, o, targetX);
  o = writeInt16(v, o, targetY);
  return buf;
}

/**
 * Encode a TOGGLE_RUN message (client → server).
 * @param {boolean} running
 * @returns {ArrayBuffer}
 */
function encodeToggleRun(running) {
  const payloadSize = 1;
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 2);
  o = writeUint16(v, o, payloadSize);
  o = writeUint8(v, o, running ? 1 : 0);
  return buf;
}

/**
 * Encode a TICK_STATE message (server → client).
 * @param {number} tickNumber
 * @param {{ id: number, tileX: number, tileY: number, running: boolean }[]} players
 * @returns {ArrayBuffer}
 */
function encodeTickState(tickNumber, players) {
  const perPlayer = 7; // u16 + i16 + i16 + u8
  const payloadSize = 4 + 2 + players.length * perPlayer; // u32 tick + u16 count + players
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 10);             // msg id
  o = writeUint16(v, o, payloadSize);
  o = writeUint32(v, o, tickNumber);
  o = writeUint16(v, o, players.length);
  for (const p of players) {
    o = writeUint16(v, o, p.id);
    o = writeInt16(v, o, p.tileX);
    o = writeInt16(v, o, p.tileY);
    o = writeUint8(v, o, p.running ? 1 : 0);
  }
  return buf;
}

/**
 * Encode a PLAYER_JOIN message (server → client).
 * @param {number} playerId
 * @param {number} tileX
 * @param {number} tileY
 * @returns {ArrayBuffer}
 */
function encodePlayerJoin(playerId, tileX, tileY) {
  const payloadSize = 6; // u16 + i16 + i16
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 11);
  o = writeUint16(v, o, payloadSize);
  o = writeUint16(v, o, playerId);
  o = writeInt16(v, o, tileX);
  o = writeInt16(v, o, tileY);
  return buf;
}

/**
 * Encode a PLAYER_LEAVE message (server → client).
 * @param {number} playerId
 * @returns {ArrayBuffer}
 */
function encodePlayerLeave(playerId) {
  const payloadSize = 2;
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 12);
  o = writeUint16(v, o, payloadSize);
  o = writeUint16(v, o, playerId);
  return buf;
}

/**
 * Encode a WELCOME message (server → client).
 * Now only contains the player ID — player starts in lobby, not in a room.
 * @param {number} playerId
 * @returns {ArrayBuffer}
 */
function encodeWelcome(playerId) {
  const payloadSize = 2;
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 13);
  o = writeUint16(v, o, payloadSize);
  o = writeUint16(v, o, playerId);
  return buf;
}

/**
 * Encode a ROOM_LIST message (server → client).
 * @param {{ id: string, playerCount: number, gridX: number, gridY: number }[]} rooms
 * @returns {ArrayBuffer}
 */
function encodeRoomList(rooms) {
  let payloadSize = 1; // u8 room count
  for (const r of rooms) {
    payloadSize += stringSize(r.id) + 2 + 1 + 1; // string + u16 count + i8 gx + i8 gy
  }
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 20);
  o = writeUint16(v, o, payloadSize);
  o = writeUint8(v, o, rooms.length);
  for (const r of rooms) {
    o = writeString(v, o, r.id);
    o = writeUint16(v, o, r.playerCount);
    o = writeInt8(v, o, r.gridX);
    o = writeInt8(v, o, r.gridY);
  }
  return buf;
}

/**
 * Encode a ROOM_JOINED message (server → client).
 * @param {string} roomId
 * @param {number} spawnX
 * @param {number} spawnY
 * @param {{ id: number, tileX: number, tileY: number }[]} existingPlayers
 * @returns {ArrayBuffer}
 */
function encodeRoomJoined(roomId, spawnX, spawnY, existingPlayers) {
  const perPlayer = 6; // u16 + i16 + i16
  const payloadSize = stringSize(roomId) + 2 + 2 + 2 + existingPlayers.length * perPlayer;
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 21);
  o = writeUint16(v, o, payloadSize);
  o = writeString(v, o, roomId);
  o = writeInt16(v, o, spawnX);
  o = writeInt16(v, o, spawnY);
  o = writeUint16(v, o, existingPlayers.length);
  for (const p of existingPlayers) {
    o = writeUint16(v, o, p.id);
    o = writeInt16(v, o, p.tileX);
    o = writeInt16(v, o, p.tileY);
  }
  return buf;
}

/**
 * Encode a ROOM_TRANSFER message (server → client).
 * @param {string} destRoomId
 * @param {number} transferTimeMs
 * @returns {ArrayBuffer}
 */
function encodeRoomTransfer(destRoomId, transferTimeMs) {
  const payloadSize = stringSize(destRoomId) + 2;
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 22);
  o = writeUint16(v, o, payloadSize);
  o = writeString(v, o, destRoomId);
  o = writeUint16(v, o, transferTimeMs);
  return buf;
}

// ── Decode ───────────────────────────────────────────────────────────────────

/**
 * Decode a single message from a binary buffer.
 * @param {ArrayBuffer|Buffer} data
 * @returns {{ type: number, payload: object }}
 */
function decode(data) {
  const buf = data instanceof ArrayBuffer ? data : data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
  const v = new DataView(buf);
  let o = 0;

  const type = v.getUint8(o); o += 1;
  const length = v.getUint16(o, true); o += 2;

  switch (type) {
    case 1: { // MOVE_TO
      const r1 = readInt16(v, o);  o = r1.offset;
      const r2 = readInt16(v, o);  o = r2.offset;
      return { type, payload: { targetX: r1.value, targetY: r2.value } };
    }
    case 2: { // TOGGLE_RUN
      const r = readUint8(v, o); o = r.offset;
      return { type, payload: { running: r.value === 1 } };
    }
    case 10: { // TICK_STATE
      const rTick = readUint32(v, o);    o = rTick.offset;
      const rCount = readUint16(v, o);   o = rCount.offset;
      const players = [];
      for (let i = 0; i < rCount.value; i++) {
        const rId = readUint16(v, o);    o = rId.offset;
        const rX = readInt16(v, o);      o = rX.offset;
        const rY = readInt16(v, o);      o = rY.offset;
        const rF = readUint8(v, o);      o = rF.offset;
        players.push({ id: rId.value, tileX: rX.value, tileY: rY.value, running: (rF.value & 1) === 1 });
      }
      return { type, payload: { tickNumber: rTick.value, players } };
    }
    case 11: { // PLAYER_JOIN
      const rId = readUint16(v, o);  o = rId.offset;
      const rX = readInt16(v, o);    o = rX.offset;
      const rY = readInt16(v, o);    o = rY.offset;
      return { type, payload: { playerId: rId.value, tileX: rX.value, tileY: rY.value } };
    }
    case 12: { // PLAYER_LEAVE
      const rId = readUint16(v, o); o = rId.offset;
      return { type, payload: { playerId: rId.value } };
    }
    case 13: { // WELCOME
      const rId = readUint16(v, o);  o = rId.offset;
      return { type, payload: { playerId: rId.value } };
    }
    case 3: { // JOIN_ROOM
      const rRoom = readString(v, o); o = rRoom.offset;
      return { type, payload: { roomId: rRoom.value } };
    }
    case 4: { // LEAVE_ROOM
      return { type, payload: {} };
    }
    case 5: { // TRANSFER_REQUEST
      const rDir = readString(v, o); o = rDir.offset;
      return { type, payload: { direction: rDir.value } };
    }
    case 20: { // ROOM_LIST
      const rCount = readUint8(v, o); o = rCount.offset;
      const rooms = [];
      for (let i = 0; i < rCount.value; i++) {
        const rRid = readString(v, o);  o = rRid.offset;
        const rPc = readUint16(v, o);   o = rPc.offset;
        const rGx = readInt8(v, o);     o = rGx.offset;
        const rGy = readInt8(v, o);     o = rGy.offset;
        rooms.push({ roomId: rRid.value, playerCount: rPc.value, gridX: rGx.value, gridY: rGy.value });
      }
      return { type, payload: { rooms } };
    }
    case 21: { // ROOM_JOINED
      const rRid = readString(v, o);  o = rRid.offset;
      const rSx = readInt16(v, o);    o = rSx.offset;
      const rSy = readInt16(v, o);    o = rSy.offset;
      const rPc = readUint16(v, o);   o = rPc.offset;
      const players = [];
      for (let i = 0; i < rPc.value; i++) {
        const rPid = readUint16(v, o); o = rPid.offset;
        const rPx = readInt16(v, o);   o = rPx.offset;
        const rPy = readInt16(v, o);   o = rPy.offset;
        players.push({ playerId: rPid.value, tileX: rPx.value, tileY: rPy.value });
      }
      return { type, payload: { roomId: rRid.value, spawnX: rSx.value, spawnY: rSy.value, players } };
    }
    case 22: { // ROOM_TRANSFER
      const rDest = readString(v, o);  o = rDest.offset;
      const rTime = readUint16(v, o);  o = rTime.offset;
      return { type, payload: { destRoomId: rDest.value, transferTimeMs: rTime.value } };
    }
    default:
      // Unknown message — skip payload (TLV forward compatibility).
      return { type, payload: null };
  }
}

module.exports = {
  HEADER_SIZE,
  encodeMoveTO,
  encodeToggleRun,
  encodeTickState,
  encodePlayerJoin,
  encodePlayerLeave,
  encodeWelcome,
  encodeRoomList,
  encodeRoomJoined,
  encodeRoomTransfer,
  decode,
};
