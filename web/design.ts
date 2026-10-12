import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";

type Vec = [number, number, number];

/** pcb/<name>.board.json, written by pcb_build: the board and each part's 3D box in the board's frame. */
export interface BoardBoxes {
  size: Vec;
  center: [number, number];
  components: { name: string; layer: string; min: Vec; max: Vec }[];
}

export interface FitIssue {
  board: string;
  item: string;
  part: string;
  status: "collision" | "tight";
}

export interface AssemblyPart {
  file: string;
  data: ArrayBuffer;
  position: Vec;
  rotation: Vec;
  color?: string;
  opacity?: number;
  /** Set for a board from pcb_build. */
  board?: BoardBoxes;
}

/** Identifies one part on one board, e.g. "pcb/main.glb|J1". */
export const partKey = (board: string, item: string) => `${board}|${item}`;

const COLORS = { collision: 0xe5604b, tight: 0xe5a54b, selected: 0x8b7cf6 };

export interface AssemblyView {
  select(key: string | null): void;
  setSeeThrough(on: boolean): void;
  dispose(): void;
}

/**
 * The 3D half of the Design view: boards and mechanical parts in place, each board part's box drawn
 * when it collides, is tight, or is selected. Clicking a part on a board calls onPick with its key.
 */
export async function showAssembly(host: HTMLElement, parts: AssemblyPart[], issues: FitIssue[], onPick: (key: string | null) => void): Promise<AssemblyView> {
  const width = host.clientWidth || 400;
  const height = host.clientHeight || 300;
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.setSize(width, height);
  host.replaceChildren(renderer.domElement);

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(40, width / height, 0.1, 100_000);
  camera.up.set(0, 0, 1);

  const cases: { material: THREE.MeshStandardMaterial; opacity?: number }[] = [];
  const boxes = new Map<string, THREE.LineSegments>();
  const pickables: THREE.Mesh[] = [];
  const status = new Map(issues.map((i) => [partKey(i.board, i.item), i.status]));
  const root = new THREE.Group();

  for (const part of parts) {
    const holder = new THREE.Group();
    holder.position.set(...part.position);
    holder.rotation.set(...(part.rotation.map((d) => (d * Math.PI) / 180) as Vec), "XYZ");
    if (part.file.toLowerCase().endsWith(".glb")) {
      const gltf = await new GLTFLoader().parseAsync(part.data, "");
      gltf.scene.rotation.x = Math.PI / 2; // glTF is Y-up; Forge is Z-up.
      holder.add(gltf.scene);
    } else {
      const geometry = new STLLoader().parse(part.data);
      geometry.computeVertexNormals();
      const material = new THREE.MeshStandardMaterial({ color: part.color ?? "#9aa3b5", metalness: 0.05, roughness: 0.55, side: THREE.DoubleSide });
      cases.push({ material, opacity: part.opacity });
      holder.add(new THREE.Mesh(geometry, material));
    }
    for (const c of part.board?.components ?? []) {
      const size = c.max.map((v, i) => Math.max(v - c.min[i], 0.05)) as Vec;
      const center = c.max.map((v, i) => (v + c.min[i]) / 2) as Vec;
      const key = partKey(part.file, c.name);
      const geometry = new THREE.BoxGeometry(...size);
      const lines = new THREE.LineSegments(new THREE.EdgesGeometry(geometry), new THREE.LineBasicMaterial({ color: COLORS.selected, depthTest: false, transparent: true }));
      lines.position.set(...center);
      lines.renderOrder = 10;
      boxes.set(key, lines);
      holder.add(lines);
      const hit = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({ visible: false }));
      hit.position.set(...center);
      hit.userData.key = key;
      pickables.push(hit);
      holder.add(hit);
    }
    root.add(holder);
  }
  scene.add(root);

  let selected: string | null = null;
  const paint = () => {
    for (const [key, lines] of boxes) {
      const s = key === selected ? "selected" : status.get(key);
      lines.visible = !!s;
      if (s) (lines.material as THREE.LineBasicMaterial).color.setHex(COLORS[s]);
    }
  };
  const setSeeThrough = (on: boolean) => {
    for (const c of cases) {
      const opacity = c.opacity ?? (on ? 0.3 : 1);
      c.material.opacity = opacity;
      c.material.transparent = opacity < 1;
      c.material.depthWrite = opacity >= 1;
      c.material.needsUpdate = true;
    }
  };
  paint();
  setSeeThrough(true);

  const box = new THREE.Box3().setFromObject(root);
  const center = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3()).length() || 10;
  const grid = new THREE.GridHelper(size * 2, 20, 0x555566, 0x2a2a36);
  grid.rotation.x = Math.PI / 2;
  grid.position.set(center.x, center.y, box.min.z - 0.01);
  scene.add(grid);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x222233, 1.3));
  const sun = new THREE.DirectionalLight(0xffffff, 1.6);
  sun.position.set(size, -size, size * 2);
  scene.add(sun);
  const fill = new THREE.DirectionalLight(0xffffff, 0.5);
  fill.position.set(-size, size, size);
  scene.add(fill);

  camera.near = size / 1000;
  camera.far = size * 100;
  camera.updateProjectionMatrix();
  camera.position.set(center.x + size * 0.7, center.y - size * 0.9, center.z + size * 0.7);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.copy(center);
  controls.update();

  // A click (not a drag) on a board part selects it.
  let down: { x: number; y: number } | null = null;
  renderer.domElement.addEventListener("pointerdown", (e) => (down = { x: e.clientX, y: e.clientY }));
  renderer.domElement.addEventListener("pointerup", (e) => {
    if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 4) return;
    const rect = renderer.domElement.getBoundingClientRect();
    const ray = new THREE.Raycaster();
    ray.setFromCamera(new THREE.Vector2(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1), camera);
    const hit = ray.intersectObjects(pickables, false)[0];
    onPick(hit ? (hit.object.userData.key as string) : null);
  });

  const resize = new ResizeObserver(() => {
    const w = host.clientWidth, h = host.clientHeight;
    if (!w || !h) return;
    renderer.setSize(w, h);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  });
  resize.observe(host);

  let frame = 0;
  const tick = () => {
    controls.update();
    renderer.render(scene, camera);
    frame = requestAnimationFrame(tick);
  };
  tick();

  return {
    select(key) {
      selected = key;
      paint();
    },
    setSeeThrough,
    dispose() {
      cancelAnimationFrame(frame);
      resize.disconnect();
      controls.dispose();
      scene.traverse((o) => {
        if (o instanceof THREE.Mesh || o instanceof THREE.LineSegments) o.geometry.dispose();
      });
      renderer.dispose();
    },
  };
}

const SVG = "http://www.w3.org/2000/svg";

export interface BoardView {
  select(name: string | null): void;
  dispose(): void;
}

/**
 * The 2D half: a PCB or schematic SVG with pan (drag) and zoom (scroll). With board data, parts are
 * outlined in their fit status, the selected one highlighted, and a click on a part calls onPick.
 */
export function showBoardSvg(host: HTMLElement, svgText: string, board: BoardBoxes | undefined, issues: Map<string, string>, onPick: (name: string | null) => void): BoardView {
  const doc = new DOMParser().parseFromString(svgText, "image/svg+xml");
  const src = doc.documentElement;
  const vb = src.getAttribute("viewBox")?.split(/[\s,]+/).map(Number);
  const W = vb?.[2] || parseFloat(src.getAttribute("width") ?? "") || 1200;
  const H = vb?.[3] || parseFloat(src.getAttribute("height") ?? "") || 900;
  const url = URL.createObjectURL(new Blob([svgText], { type: "image/svg+xml" }));

  const svg = document.createElementNS(SVG, "svg");
  let view = { x: 0, y: 0, w: W, h: H };
  const setView = () => svg.setAttribute("viewBox", `${view.x} ${view.y} ${view.w} ${view.h}`);
  setView();
  const image = document.createElementNS(SVG, "image");
  image.setAttribute("href", url);
  image.setAttribute("width", String(W));
  image.setAttribute("height", String(H));
  svg.append(image);
  host.replaceChildren(svg);

  // Board millimetres -> SVG units, from where the SVG drew the board's outline.
  const outline = src.querySelector('[data-type="pcb_board"]')?.getAttribute("d");
  const marks = new Map<string, SVGRectElement>();
  let toSvg: ((x: number, y: number) => [number, number]) | undefined;
  if (board && outline) {
    const n = (outline.match(/-?\d+(\.\d+)?/g) ?? []).map(Number);
    const xs = n.filter((_, i) => i % 2 === 0), ys = n.filter((_, i) => i % 2 === 1);
    const [x0, x1, y0] = [Math.min(...xs), Math.max(...xs), Math.min(...ys)];
    const s = (x1 - x0) / board.size[0];
    const left = board.center[0] - board.size[0] / 2, top = board.center[1] + board.size[1] / 2;
    toSvg = (x, y) => [x0 + (x - left) * s, y0 + (top - y) * s];
    for (const c of board.components) {
      const [ax, ay] = toSvg(c.min[0], c.max[1]);
      const [bx, by] = toSvg(c.max[0], c.min[1]);
      const r = document.createElementNS(SVG, "rect");
      Object.entries({ x: ax - 3, y: ay - 3, width: bx - ax + 6, height: by - ay + 6, rx: 4, fill: "none", "stroke-width": 3 }).forEach(([k, v]) => r.setAttribute(k, String(v)));
      marks.set(c.name, r);
      svg.append(r);
    }
  }
  let selected: string | null = null;
  const paint = () => {
    for (const [name, r] of marks) {
      const s = name === selected ? "selected" : issues.get(name);
      r.setAttribute("stroke", s === "selected" ? "#8b7cf6" : s === "collision" ? "#e5604b" : s === "tight" ? "#e5a54b" : "none");
      r.setAttribute("fill", s === "selected" ? "rgb(139 124 246 / 0.18)" : "none");
    }
  };
  paint();

  const toUser = (e: MouseEvent) => {
    const rect = svg.getBoundingClientRect();
    // preserveAspectRatio xMidYMid meet: one scale, centred.
    const k = Math.min(rect.width / view.w, rect.height / view.h);
    return [view.x + (e.clientX - rect.left - (rect.width - view.w * k) / 2) / k, view.y + (e.clientY - rect.top - (rect.height - view.h * k) / 2) / k, k];
  };
  svg.addEventListener("wheel", (e) => {
    e.preventDefault();
    const [ux, uy] = toUser(e);
    const f = Math.exp(e.deltaY * 0.0015);
    const w = Math.min(Math.max(view.w * f, W / 40), W * 4);
    const g = w / view.w;
    view = { x: ux - (ux - view.x) * g, y: uy - (uy - view.y) * g, w, h: view.h * g };
    setView();
  }, { passive: false });
  let drag: { x: number; y: number; vx: number; vy: number; moved: boolean } | null = null;
  svg.addEventListener("pointerdown", (e) => {
    drag = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y, moved: false };
    svg.setPointerCapture(e.pointerId);
  });
  svg.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const k = toUser(e)[2];
    if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) > 3) drag.moved = true;
    if (!drag.moved) return;
    svg.classList.add("panning");
    view.x = drag.vx - (e.clientX - drag.x) / k;
    view.y = drag.vy - (e.clientY - drag.y) / k;
    setView();
  });
  svg.addEventListener("pointerup", (e) => {
    svg.classList.remove("panning");
    const click = drag && !drag.moved;
    drag = null;
    if (!click || !toSvg || !board) return;
    const [ux, uy] = toUser(e);
    // The smallest part under the pointer wins, so a resistor inside a connector's box can be picked.
    const under = board.components
      .filter((c) => {
        const [ax, ay] = toSvg!(c.min[0], c.max[1]);
        const [bx, by] = toSvg!(c.max[0], c.min[1]);
        return ux >= ax - 3 && ux <= bx + 3 && uy >= ay - 3 && uy <= by + 3;
      })
      .sort((a, b) => (a.max[0] - a.min[0]) * (a.max[1] - a.min[1]) - (b.max[0] - b.min[0]) * (b.max[1] - b.min[1]));
    onPick(under[0]?.name ?? null);
  });
  svg.addEventListener("dblclick", () => {
    view = { x: 0, y: 0, w: W, h: H };
    setView();
  });

  return {
    select(name) {
      selected = name;
      paint();
    },
    dispose() {
      URL.revokeObjectURL(url);
    },
  };
}
