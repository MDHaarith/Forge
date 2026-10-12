import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";

/** Parses a model file into a scene object. GLB keeps its own materials; STL gets the accent colour. */
async function load(data: ArrayBuffer, format: "stl" | "glb"): Promise<THREE.Object3D> {
  if (format === "glb") {
    const gltf = await new GLTFLoader().parseAsync(data, "");
    // glTF is Y-up; turn it to the Z-up convention Forge and CAD use.
    gltf.scene.rotation.x = Math.PI / 2;
    gltf.scene.updateMatrixWorld(true);
    return gltf.scene;
  }
  const geometry = new STLLoader().parse(data);
  geometry.computeVertexNormals();
  const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#8b7cf6";
  return new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({ color: accent, metalness: 0.1, roughness: 0.6 }));
}

/** Renders an STL or GLB into `host` with orbit controls. Returns a function that tears the view down. */
export async function showModel(host: HTMLElement, data: ArrayBuffer, format: "stl" | "glb"): Promise<() => void> {
  const model = await load(data, format);
  const width = host.clientWidth || 320;
  const height = host.clientHeight || 280;
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.setSize(width, height);
  host.replaceChildren(renderer.domElement);

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(40, width / height, 0.1, 100_000);
  camera.up.set(0, 0, 1); // CAD convention: Z up.

  const box = new THREE.Box3().setFromObject(model);
  const center = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3()).length() || 1;
  scene.add(model);
  const grid = new THREE.GridHelper(size * 2, 20, 0x555566, 0x333344);
  grid.rotation.x = Math.PI / 2;
  grid.position.set(center.x, center.y, box.min.z);
  scene.add(grid);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x222233, 1.2));
  const sun = new THREE.DirectionalLight(0xffffff, 1.5);
  sun.position.set(size, -size, size * 2);
  scene.add(sun);

  camera.near = size / 1000;
  camera.far = size * 100;
  camera.updateProjectionMatrix();
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
    model.traverse((o) => {
      if (o instanceof THREE.Mesh) o.geometry.dispose();
    });
    renderer.dispose();
  };
}
