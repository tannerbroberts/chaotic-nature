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
 * @param {number} playerId
 * @param {number} tileX
 * @param {number} tileY
 * @returns {ArrayBuffer}
 */
function encodeWelcome(playerId, tileX, tileY) {
  const payloadSize = 6;
  const buf = new ArrayBuffer(HEADER_SIZE + payloadSize);
  const v = new DataView(buf);
  let o = 0;
  o = writeUint8(v, o, 13);
  o = writeUint16(v, o, payloadSize);
  o = writeUint16(v, o, playerId);
  o = writeInt16(v, o, tileX);
  o = writeInt16(v, o, tileY);
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
      const rX = readInt16(v, o);    o = rX.offset;
      const rY = readInt16(v, o);    o = rY.offset;
      return { type, payload: { playerId: rId.value, tileX: rX.value, tileY: rY.value } };
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
  decode,
};
