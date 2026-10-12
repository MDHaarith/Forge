/** Geometry for the board-in-enclosure fit check: STL parsing, placements and triangle-box overlap. */

export type Vec = [number, number, number];

export interface Placement {
  /** Millimetres. */
  position: Vec;
  /** Degrees about X, then Y, then Z of the rotated frame (three.js "XYZ" order, as the 3D view uses). */
  rotation: Vec;
}

/** Triangles of an STL file, binary or ASCII, as 9 numbers per triangle. */
export function parseStl(data: Buffer): Float32Array {
  if (data.length >= 84) {
    const count = data.readUInt32LE(80);
    if (84 + count * 50 === data.length) {
      const out = new Float32Array(count * 9);
      for (let i = 0; i < count; i++) for (let k = 0; k < 9; k++) out[i * 9 + k] = data.readFloatLE(84 + i * 50 + 12 + k * 4);
      return out;
    }
  }
  const numbers: number[] = [];
  for (const m of data.toString("latin1").matchAll(/vertex\s+(\S+)\s+(\S+)\s+(\S+)/g)) numbers.push(+m[1], +m[2], +m[3]);
  if (!numbers.length || numbers.length % 9 || numbers.some(Number.isNaN)) throw new Error("not an STL file");
  return Float32Array.from(numbers);
}

/** Row-major 3×3 rotation for a placement: Rx · Ry · Rz. */
export function rotationMatrix([ax, ay, az]: Vec): number[] {
  const [a, b, c] = [ax, ay, az].map((d) => (d * Math.PI) / 180);
  const [ca, sa, cb, sb, cc, sc] = [Math.cos(a), Math.sin(a), Math.cos(b), Math.sin(b), Math.cos(c), Math.sin(c)];
  return [
    cb * cc, -cb * sc, sb,
    ca * sc + sa * sb * cc, ca * cc - sa * sb * sc, -sa * cb,
    sa * sc - ca * sb * cc, sa * cc + ca * sb * sc, ca * cb,
  ];
}

export function apply(m: number[], t: Vec, p: Vec): Vec {
  return [m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + t[0], m[3] * p[0] + m[4] * p[1] + m[5] * p[2] + t[1], m[6] * p[0] + m[7] * p[1] + m[8] * p[2] + t[2]];
}

/** The inverse of apply(m, t, ·) for a rotation m. */
export function unapply(m: number[], t: Vec, p: Vec): Vec {
  const q: Vec = [p[0] - t[0], p[1] - t[1], p[2] - t[2]];
  return [m[0] * q[0] + m[3] * q[1] + m[6] * q[2], m[1] * q[0] + m[4] * q[1] + m[7] * q[2], m[2] * q[0] + m[5] * q[1] + m[8] * q[2]];
}

/** Separating-axis test of a triangle against an axis-aligned box given by centre and half sizes (Akenine-Möller). */
export function triangleHitsBox(c: Vec, h: Vec, a: Vec, b: Vec, d: Vec): boolean {
  const v = [a, b, d].map((p) => [p[0] - c[0], p[1] - c[1], p[2] - c[2]] as Vec);
  for (let i = 0; i < 3; i++) {
    const lo = Math.min(v[0][i], v[1][i], v[2][i]);
    const hi = Math.max(v[0][i], v[1][i], v[2][i]);
    if (lo > h[i] || hi < -h[i]) return false;
  }
  const e = [0, 1, 2].map((i) => {
    const p = v[i], q = v[(i + 1) % 3];
    return [q[0] - p[0], q[1] - p[1], q[2] - p[2]] as Vec;
  });
  // The nine cross products of the box axes with the triangle edges.
  for (const edge of e) {
    const axes: Vec[] = [[0, -edge[2], edge[1]], [edge[2], 0, -edge[0]], [-edge[1], edge[0], 0]];
    for (const ax of axes) {
      const p = v.map((x) => x[0] * ax[0] + x[1] * ax[1] + x[2] * ax[2]);
      const r = h[0] * Math.abs(ax[0]) + h[1] * Math.abs(ax[1]) + h[2] * Math.abs(ax[2]);
      if (Math.min(...p) > r || Math.max(...p) < -r) return false;
    }
  }
  // The triangle's plane.
  const n: Vec = [e[0][1] * e[1][2] - e[0][2] * e[1][1], e[0][2] * e[1][0] - e[0][0] * e[1][2], e[0][0] * e[1][1] - e[0][1] * e[1][0]];
  const dist = n[0] * v[0][0] + n[1] * v[0][1] + n[2] * v[0][2];
  const r = h[0] * Math.abs(n[0]) + h[1] * Math.abs(n[1]) + h[2] * Math.abs(n[2]);
  return Math.abs(dist) <= r;
}

export interface BoardBoxes {
  size: Vec;
  center: [number, number];
  components: { name: string; layer: string; min: Vec; max: Vec }[];
}

export interface FitIssue {
  board: string;
  /** "board" for the board itself, else a part reference such as J1. */
  item: string;
  part: string;
  status: "collision" | "tight";
}

/** Items of a board to check, in the board's own frame: the board itself, then every part on it. */
export function boardItems(board: BoardBoxes): { item: string; min: Vec; max: Vec }[] {
  const [w, h, t] = board.size;
  const [cx, cy] = board.center;
  return [{ item: "board", min: [cx - w / 2, cy - h / 2, -t / 2], max: [cx + w / 2, cy + h / 2, t / 2] }, ...board.components.map((c) => ({ item: c.name, min: c.min, max: c.max }))];
}

/**
 * Which parts of each board touch an enclosure solid. A box that cuts into a solid is a collision; one
 * that only comes within `clearance` is tight. The board itself may rest on standoffs, so it only
 * collides when it sinks more than 0.05 mm into a solid.
 */
export function fitCheck(
  boards: { name: string; placement: Placement; board: BoardBoxes }[],
  solids: { name: string; placement: Placement; triangles: Float32Array }[],
  clearance: number,
): FitIssue[] {
  const issues: FitIssue[] = [];
  for (const b of boards) {
    const bm = rotationMatrix(b.placement.rotation);
    const items = boardItems(b.board);
    for (const s of solids) {
      const sm = rotationMatrix(s.placement.rotation);
      // Every triangle in the board's frame, once per pair.
      const tris: Vec[][] = [];
      for (let i = 0; i < s.triangles.length; i += 9) {
        const corner = (k: number) => unapply(bm, b.placement.position, apply(sm, s.placement.position, [s.triangles[i + k], s.triangles[i + k + 1], s.triangles[i + k + 2]]));
        tris.push([corner(0), corner(3), corner(6)]);
      }
      for (const it of items) {
        const shrink = it.item === "board" ? 0.05 : 0.001;
        const test = (grow: number) => {
          const c: Vec = [0, 1, 2].map((i) => (it.min[i] + it.max[i]) / 2) as Vec;
          const h: Vec = [0, 1, 2].map((i) => Math.max((it.max[i] - it.min[i]) / 2 + grow, 1e-6)) as Vec;
          return tris.some(([p, q, r]) => triangleHitsBox(c, h, p, q, r));
        };
        if (test(-shrink)) issues.push({ board: b.name, item: it.item, part: s.name, status: "collision" });
        else if (clearance > 0 && test(clearance)) issues.push({ board: b.name, item: it.item, part: s.name, status: "tight" });
      }
    }
  }
  return issues;
}

/** The world-space bounds of a board with its parts, for sizing an enclosure around it. */
export function boardBounds(board: BoardBoxes, placement: Placement): { min: Vec; max: Vec } {
  const m = rotationMatrix(placement.rotation);
  const min: Vec = [Infinity, Infinity, Infinity];
  const max: Vec = [-Infinity, -Infinity, -Infinity];
  for (const it of boardItems(board)) {
    for (let corner = 0; corner < 8; corner++) {
      const p = apply(m, placement.position, [0, 1, 2].map((i) => ((corner >> i) & 1 ? it.max[i] : it.min[i])) as Vec);
      for (let i = 0; i < 3; i++) {
        min[i] = Math.min(min[i], p[i]);
        max[i] = Math.max(max[i], p[i]);
      }
    }
  }
  return { min, max };
}
