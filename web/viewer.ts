import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";

/** Renders an STL into `host` with orbit controls. Returns a function that tears the view down. */
export function showStl(host: HTMLElement, data: ArrayBuffer): () => void {
  const width = host.clientWidth || 320;
  const height = host.clientHeight || 280;
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.setSize(width, height);
  host.replaceChildren(renderer.domElement);

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(40, width / height, 0.1, 100_000);
  camera.up.set(0, 0, 1); // CAD convention: Z up.

  const geometry = new STLLoader().parse(data);
  geometry.computeVertexNormals();
  geometry.computeBoundingBox();
  const box = geometry.boundingBox!;
  const center = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3()).length() || 1;

  const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#8b7cf6";
  const mesh = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({ color: accent, metalness: 0.1, roughness: 0.6 }));
  scene.add(mesh);
  const grid = new THREE.GridHelper(size * 2, 20, 0x555566, 0x333344);
  grid.rotation.x = Math.PI / 2;
  grid.position.set(center.x, center.y, box.min.z);
  scene.add(grid);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x222233, 1.2));
  const sun = new THREE.DirectionalLight(0xffffff, 1.5);
  sun.position.set(size, -size, size * 2);
  scene.add(sun);

  camera.position.set(center.x + size, center.y - size, center.z + size * 0.8);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.copy(center);
  controls.update();

  let frame = 0;
  const tick = () => {
    controls.update();
    renderer.render(scene, camera);
    frame = requestAnimationFrame(tick);
  };
  tick();

  return () => {
    cancelAnimationFrame(frame);
    controls.dispose();
    geometry.dispose();
    renderer.dispose();
  };
}
