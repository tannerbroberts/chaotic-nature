/**
 * pathfinder.js — BFS tile pathfinder for the Chaotic Nature server.
 *
 * Direct port of scripts/pathfinder.gd. Uses the same collision flag layout
 * and movement rules (Chebyshev 8-directional, diagonal corner-cutting blocked
 * by adjacent solid tiles).
 *
 * Collision flag bit layout (per tile):
 *   N=1  NE=2  E=4  SE=8  S=16  SW=32  W=64  NW=128
 */

'use strict';

/** Direction offsets with exit/entry flag pairs. */
const DIRECTIONS = [
  { dx:  0, dy: -1, exit: 1,   entry: 16  }, // N
  { dx:  1, dy: -1, exit: 2,   entry: 32  }, // NE
  { dx:  1, dy:  0, exit: 4,   entry: 64  }, // E
  { dx:  1, dy:  1, exit: 8,   entry: 128 }, // SE
  { dx:  0, dy:  1, exit: 16,  entry: 1   }, // S
  { dx: -1, dy:  1, exit: 32,  entry: 2   }, // SW
  { dx: -1, dy:  0, exit: 64,  entry: 4   }, // W
  { dx: -1, dy: -1, exit: 128, entry: 8   }, // NW
];

class Pathfinder {
  /**
   * @param {number} width  — grid width in tiles
   * @param {number} height — grid height in tiles
   * @param {Int32Array|number[]} collisionFlags — flat array [y * width + x] of per-tile flags
   */
  constructor(width, height, collisionFlags) {
    this.width = width;
    this.height = height;
    this.flags = collisionFlags;
  }

  /** Get collision flags for a tile. Out-of-bounds → 255 (fully blocked). */
  getFlags(x, y) {
    if (x < 0 || y < 0 || x >= this.width || y >= this.height) return 255;
    return this.flags[y * this.width + x];
  }

  /** Check whether a single step from (fx,fy) to (tx,ty) is walkable. */
  canStep(fx, fy, tx, ty) {
    const dx = tx - fx;
    const dy = ty - fy;
    for (const d of DIRECTIONS) {
      if (d.dx === dx && d.dy === dy) {
        if (this.getFlags(fx, fy) & d.exit) return false;
        if (this.getFlags(tx, ty) & d.entry) return false;
        // Diagonal corner-cut check.
        if (dx !== 0 && dy !== 0) {
          if (this.getFlags(fx + dx, fy) === 255) return false;
          if (this.getFlags(fx, fy + dy) === 255) return false;
        }
        return true;
      }
    }
    return false;
  }

  /**
   * Try a straight-line path favouring the larger axis.
   * Returns array of {x,y} (start exclusive, end inclusive), or null if blocked.
   */
  _directPath(sx, sy, ex, ey) {
    const path = [];
    let cx = sx, cy = sy;
    while (cx !== ex || cy !== ey) {
      const dx = ex - cx;
      const dy = ey - cy;
      const ax = Math.abs(dx);
      const ay = Math.abs(dy);
      let stepX, stepY;
      if (ax > ay)       { stepX = Math.sign(dx); stepY = 0; }
      else if (ay > ax)  { stepX = 0; stepY = Math.sign(dy); }
      else               { stepX = Math.sign(dx); stepY = Math.sign(dy); }
      const nx = cx + stepX;
      const ny = cy + stepY;
      if (!this.canStep(cx, cy, nx, ny)) return null;
      cx = nx;
      cy = ny;
      path.push({ x: cx, y: cy });
    }
    return path;
  }

  /**
   * BFS fallback when the direct path is blocked.
   * Returns array of {x,y} (start exclusive, end inclusive), or empty if unreachable.
   */
  _bfsPath(sx, sy, ex, ey) {
    const key = (x, y) => y * this.width + x;
    const startKey = key(sx, sy);
    const endKey = key(ex, ey);
    const cameFrom = new Map();
    cameFrom.set(startKey, null);
    const queue = [{ x: sx, y: sy }];
    let found = false;

    while (queue.length > 0) {
      const cur = queue.shift();
      if (cur.x === ex && cur.y === ey) { found = true; break; }
      for (const d of DIRECTIONS) {
        const nx = cur.x + d.dx;
        const ny = cur.y + d.dy;
        const nk = key(nx, ny);
        if (cameFrom.has(nk)) continue;
        if (nx < 0 || ny < 0 || nx >= this.width || ny >= this.height) continue;
        if (!this.canStep(cur.x, cur.y, nx, ny)) continue;
        cameFrom.set(nk, cur);
        queue.push({ x: nx, y: ny });
      }
    }

    if (!found) return [];
    const path = [];
    let trace = { x: ex, y: ey };
    while (trace.x !== sx || trace.y !== sy) {
      path.push({ x: trace.x, y: trace.y });
      trace = cameFrom.get(key(trace.x, trace.y));
    }
    path.reverse();
    return path;
  }

  /**
   * Find a path from (sx,sy) to (ex,ey).
   * @returns {{ x: number, y: number }[]} — start-exclusive, end-inclusive.
   */
  findPath(sx, sy, ex, ey) {
    if (sx === ex && sy === ey) return [];
    if (ex < 0 || ey < 0 || ex >= this.width || ey >= this.height) return [];
    const direct = this._directPath(sx, sy, ex, ey);
    if (direct) return direct;
    return this._bfsPath(sx, sy, ex, ey);
  }
}

module.exports = Pathfinder;
