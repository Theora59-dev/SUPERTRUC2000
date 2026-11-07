import { useEffect, useRef, useState, type MutableRefObject, type ReactNode } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { previewMesh, type Axis, type Bed, type PartMeta } from "./api";

export type ViewKind = "fit" | "iso" | "top" | "front" | "right";

export interface ViewCommand {
  kind: ViewKind;
  nonce: number;
}

export interface PlanePreview {
  axis: Axis;
  position: number;
  part: PartMeta;
}

export interface ViewportProps {
  parts: PartMeta[];
  bed: Bed;
  selectedId: number | null;
  hidden: Set<number>;
  plane: PlanePreview | null;
  command: ViewCommand;
  exploded: boolean;
  onSelect: (id: number | null) => void;
  children?: ReactNode;
}

const PALETTE = [
  "#4fb3a1",
  "#6f9bd1",
  "#c9a06a",
  "#9a8bd0",
  "#c27e7e",
  "#7fb37f",
  "#c98fc0",
  "#8f9fb3",
];

const SELECTED_COLOR = "#ffb224";

export function partColorAt(index: number): string {
  return PALETTE[((index % PALETTE.length) + PALETTE.length) % PALETTE.length];
}

const AXIS_INDEX: Record<Axis, number> = { x: 0, y: 1, z: 2 };

interface Entry {
  mesh: THREE.Mesh;
  material: THREE.MeshStandardMaterial;
  revision: number;
  part: PartMeta;
  index: number;
  token: number;
  explode: THREE.Vector3;
  colored: boolean;
}

interface EngineApi {
  setParts: (parts: PartMeta[], hidden: Set<number>) => void;
  setSelected: (id: number | null) => void;
  setPlane: (plane: PlanePreview | null) => void;
  setBed: (bed: Bed) => void;
  setExploded: (exploded: boolean) => void;
  view: (kind: ViewKind) => void;
  dispose: () => void;
}

function parseGeometry(buffer: ArrayBuffer): { geometry: THREE.BufferGeometry; colored: boolean } {
  const view = new DataView(buffer);
  const vertexCount = view.getUint32(0, true);
  const indexCount = view.getUint32(4, true);
  const positions = new Float32Array(buffer, 8, vertexCount * 3);
  const indexOffset = 8 + vertexCount * 12;
  const indices = new Uint32Array(buffer, indexOffset, indexCount);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geometry.setIndex(new THREE.BufferAttribute(indices, 1));
  const colorOffset = indexOffset + indexCount * 4;
  const colored = buffer.byteLength >= colorOffset + vertexCount * 3;
  if (colored) {
    const colors = new Uint8Array(buffer, colorOffset, vertexCount * 3);
    geometry.setAttribute(
      "color",
      new THREE.BufferAttribute(colors, 3, true),
    );
  }
  geometry.computeVertexNormals();
  geometry.computeBoundingBox();
  geometry.computeBoundingSphere();
  return { geometry, colored };
}

function disposeMaterial(material: THREE.Material | THREE.Material[] | undefined) {
  if (!material) return;
  if (Array.isArray(material)) {
    for (const m of material) m.dispose();
  } else {
    material.dispose();
  }
}

function disposeObject(object: THREE.Object3D) {
  object.traverse((child) => {
    const mesh = child as THREE.Mesh;
    if (mesh.geometry) mesh.geometry.dispose();
    disposeMaterial(mesh.material);
  });
}

function createEngine(
  host: HTMLElement,
  selectRef: MutableRefObject<(id: number | null) => void>,
  onLoading: (count: number) => void,
): EngineApi {
  const scene = new THREE.Scene();

  const camera = new THREE.PerspectiveCamera(43, 1, 0.2, 40000);
  camera.up.set(0, 0, 1);
  camera.position.set(320, -440, 340);

  const renderer = new THREE.WebGLRenderer({
    antialias: true,
    alpha: true,
    powerPreference: "high-performance",
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.08;
  renderer.setClearColor(0x000000, 0);
  host.appendChild(renderer.domElement);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.085;
  controls.screenSpacePanning = true;
  controls.minDistance = 0.5;
  controls.maxDistance = 20000;
  controls.maxPolarAngle = Math.PI * 0.88;
  controls.target.set(0, 0, 40);
  controls.update();

  scene.add(new THREE.HemisphereLight(0xc3dcea, 0x0a0e12, 1.35));

  const keyLight = new THREE.DirectionalLight(0xffffff, 2.35);
  keyLight.position.set(420, -620, 900);
  scene.add(keyLight);

  const fillLight = new THREE.DirectionalLight(0x3ecfb2, 0.85);
  fillLight.position.set(-700, 420, 300);
  scene.add(fillLight);

  const rimLight = new THREE.DirectionalLight(0xffb224, 0.6);
  rimLight.position.set(150, 820, 420);
  scene.add(rimLight);

  const partsGroup = new THREE.Group();
  scene.add(partsGroup);

  const bedGroup = new THREE.Group();
  scene.add(bedGroup);

  const planeGroup = new THREE.Group();
  scene.add(planeGroup);

  const entries = new Map<number, Entry>();
  let currentParts: PartMeta[] = [];
  let hidden = new Set<number>();
  let selectedId: number | null = null;
  let bed: Bed = { x: 220, y: 220, z: 250, margin: 5 };
  let disposed = false;
  let tokenSeed = 0;
  let loadingCount = 0;
  let explodeTarget = 0;
  let explodeProgress = 0;
  const clock = new THREE.Clock();

  const pending: Array<{ entry: Entry; part: PartMeta; token: number }> = [];
  let pumping = false;

  const setLoading = (value: number) => {
    if (disposed) return;
    loadingCount = Math.max(0, value);
    onLoading(loadingCount);
  };

  async function pump() {
    if (pumping || disposed) return;
    pumping = true;
    while (pending.length > 0 && !disposed) {
      const job = pending.shift()!;
      if (entries.get(job.part.id) !== job.entry || job.entry.token !== job.token) continue;
      setLoading(loadingCount + 1);
      try {
        const buffer = await previewMesh(job.part.id);
        if (disposed) break;
        if (entries.get(job.part.id) !== job.entry || job.entry.token !== job.token) continue;
        const parsed = parseGeometry(buffer);
        const previous = job.entry.mesh.geometry;
        job.entry.mesh.geometry = parsed.geometry;
        job.entry.colored = parsed.colored;
        styleEntry(job.entry);
        if (previous && previous !== parsed.geometry) previous.dispose();
      } catch (error) {
        if (!disposed) console.error("Aperçu indisponible", error);
      } finally {
        setLoading(loadingCount - 1);
      }
    }
    pumping = false;
  }

  function styleEntry(entry: Entry) {
    const selected = selectedId !== null && entry.part.id === selectedId;
    if (entry.material.vertexColors !== entry.colored) {
      entry.material.vertexColors = entry.colored;
      entry.material.needsUpdate = true;
    }
    // Un modèle coloré conserve ses couleurs même sélectionné : la sélection
    // est signalée par l'émission, pas par une teinte qui fausserait la palette.
    entry.material.color.set(
      entry.colored
        ? "#ffffff"
        : selected
          ? SELECTED_COLOR
          : partColorAt(entry.index),
    );
    entry.material.emissive.set(selected ? "#583600" : "#000000");
    entry.material.emissiveIntensity = selected ? 0.9 : 0;
    entry.mesh.visible = !hidden.has(entry.part.id);
  }

  function createEntry(part: PartMeta, index: number): Entry {
    const material = new THREE.MeshStandardMaterial({
      color: new THREE.Color(partColorAt(index)),
      metalness: 0.06,
      roughness: 0.7,
      flatShading: true,
      vertexColors: false,
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
    });
    const mesh = new THREE.Mesh(new THREE.BufferGeometry(), material);
    mesh.userData.partId = part.id;
    mesh.frustumCulled = true;
    partsGroup.add(mesh);
    const entry: Entry = {
      mesh,
      material,
      revision: part.revision,
      part,
      index,
      token: 0,
      explode: new THREE.Vector3(),
      colored: false,
    };
    return entry;
  }

  // À appeler une fois l'entrée enregistrée dans `entries` : `pump` vérifie
  // cette map de façon synchrone avant de lancer la requête d'aperçu.
  function queuePreview(entry: Entry, part: PartMeta) {
    const token = ++tokenSeed;
    entry.token = token;
    pending.push({ entry, part, token });
    void pump();
  }

  function updateExplodeOffsets() {
    const parts = currentParts;
    if (parts.length === 0) return;
    const box = new THREE.Box3();
    for (const part of parts) {
      box.expandByPoint(new THREE.Vector3(part.min[0], part.min[1], part.min[2]));
      box.expandByPoint(new THREE.Vector3(part.max[0], part.max[1], part.max[2]));
    }
    const center = box.getCenter(new THREE.Vector3());
    const diagonal = box.getSize(new THREE.Vector3()).length();
    const scale = parts.length > 1 ? Math.max(diagonal * 0.28, 10) : 0;
    parts.forEach((part, index) => {
      const entry = entries.get(part.id);
      if (!entry) return;
      const direction = new THREE.Vector3(
        part.center[0],
        part.center[1],
        part.center[2],
      ).sub(center);
      if (direction.lengthSq() < 1e-9) {
        const angle = (index / Math.max(parts.length, 1)) * Math.PI * 2;
        direction.set(Math.cos(angle), Math.sin(angle), 0.4);
      }
      entry.explode.copy(direction.normalize().multiplyScalar(scale));
    });
  }

  function setParts(parts: PartMeta[], nextHidden: Set<number>) {
    currentParts = parts;
    hidden = nextHidden;
    const alive = new Set(parts.map((p) => p.id));
    for (const [id, entry] of entries) {
      if (!alive.has(id)) {
        partsGroup.remove(entry.mesh);
        entry.mesh.geometry.dispose();
        entry.material.dispose();
        entries.delete(id);
      }
    }
    parts.forEach((part, index) => {
      let entry = entries.get(part.id);
      if (!entry) {
        entry = createEntry(part, index);
        entries.set(part.id, entry);
        queuePreview(entry, part);
      } else if (entry.revision !== part.revision) {
        entry.revision = part.revision;
        entry.part = part;
        entry.index = index;
        queuePreview(entry, part);
      } else {
        entry.part = part;
        entry.index = index;
      }
      styleEntry(entry);
    });
    updateExplodeOffsets();
  }

  function setSelected(id: number | null) {
    selectedId = id;
    for (const entry of entries.values()) styleEntry(entry);
  }

  function setExploded(value: boolean) {
    explodeTarget = value ? 1 : 0;
  }

  function clearPlane() {
    for (const child of [...planeGroup.children]) {
      planeGroup.remove(child);
      disposeObject(child);
    }
  }

  function setPlane(preview: PlanePreview | null) {
    clearPlane();
    if (!preview) return;
    const { axis, position, part } = preview;
    const [cx, cy, cz] = part.center;
    const [sx, sy, sz] = part.size;
    let width = sx;
    let height = sy;
    let rotationX = 0;
    let rotationY = 0;
    let px = cx;
    let py = cy;
    let pz = cz;
    if (axis === "x") {
      width = Math.max(sz, 0.1);
      height = Math.max(sy, 0.1);
      rotationY = Math.PI / 2;
      px = position;
    } else if (axis === "y") {
      width = Math.max(sx, 0.1);
      height = Math.max(sz, 0.1);
      rotationX = -Math.PI / 2;
      py = position;
    } else {
      width = Math.max(sx, 0.1);
      height = Math.max(sy, 0.1);
      pz = position;
    }
    width *= 1.07;
    height *= 1.07;
    const geometry = new THREE.PlaneGeometry(width, height);
    const fill = new THREE.Mesh(
      geometry,
      new THREE.MeshBasicMaterial({
        color: 0xffb224,
        transparent: true,
        opacity: 0.1,
        side: THREE.DoubleSide,
        depthWrite: false,
        depthTest: false,
      }),
    );
    const border = new THREE.LineSegments(
      new THREE.EdgesGeometry(geometry),
      new THREE.LineBasicMaterial({
        color: 0xffb224,
        transparent: true,
        opacity: 0.95,
        depthTest: false,
      }),
    );
    const halfW = width / 2;
    const halfH = height / 2;
    const crossGeometry = new THREE.BufferGeometry();
    crossGeometry.setAttribute(
      "position",
      new THREE.Float32BufferAttribute(
        [
          -halfW, 0, 0.01, halfW, 0, 0.01, 0, -halfH, 0.01, 0, halfH, 0.01,
        ],
        3,
      ),
    );
    const cross = new THREE.LineSegments(
      crossGeometry,
      new THREE.LineBasicMaterial({
        color: 0xffd98a,
        transparent: true,
        opacity: 0.45,
        depthTest: false,
      }),
    );
    const group = new THREE.Group();
    group.add(fill, border, cross);
    group.position.set(px, py, pz);
    group.rotation.set(rotationX, rotationY, 0);
    group.renderOrder = 10;
    planeGroup.add(group);
  }

  function buildBed(nextBed: Bed) {
    bed = nextBed;
    for (const child of [...bedGroup.children]) {
      bedGroup.remove(child);
      disposeObject(child);
    }
    const hx = bed.x / 2;
    const hy = bed.y / 2;

    const plane = new THREE.Mesh(
      new THREE.PlaneGeometry(bed.x, bed.y),
      new THREE.MeshBasicMaterial({
        color: 0x0b1418,
        transparent: true,
        opacity: 0.94,
      }),
    );
    plane.position.z = -0.6;
    bedGroup.add(plane);

    const minor: number[] = [];
    const major: number[] = [];
    const step = Math.max(2, Math.round(Math.max(bed.x, bed.y) / 30 / 5) * 5);
    for (let x = -hx; x <= hx + 1e-6; x += step) {
      const target = Math.abs(x % (step * 5)) < 1e-6 ? major : minor;
      target.push(x, -hy, 0.06, x, hy, 0.06);
    }
    for (let y = -hy; y <= hy + 1e-6; y += step) {
      const target = Math.abs(y % (step * 5)) < 1e-6 ? major : minor;
      target.push(-hx, y, 0.06, hx, y, 0.06);
    }
    for (const [points, color] of [
      [minor, 0x18242a],
      [major, 0x25343c],
    ] as Array<[number[], number]>) {
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute("position", new THREE.Float32BufferAttribute(points, 3));
      bedGroup.add(
        new THREE.LineSegments(
          geometry,
          new THREE.LineBasicMaterial({
            color,
            transparent: true,
            opacity: 0.9,
            depthWrite: false,
          }),
        ),
      );
    }

    const borderGeometry = new THREE.BufferGeometry();
    borderGeometry.setAttribute(
      "position",
      new THREE.Float32BufferAttribute(
        [-hx, -hy, 0.1, hx, -hy, 0.1, hx, hy, 0.1, -hx, hy, 0.1],
        3,
      ),
    );
    bedGroup.add(
      new THREE.LineLoop(
        borderGeometry,
        new THREE.LineBasicMaterial({
          color: 0x3ecfb2,
          transparent: true,
          opacity: 0.55,
        }),
      ),
    );

    const axisGeometry = new THREE.BufferGeometry();
    axisGeometry.setAttribute(
      "position",
      new THREE.Float32BufferAttribute(
        [-hx, 0, 0.08, hx, 0, 0.08, 0, -hy, 0.08, 0, hy, 0.08],
        3,
      ),
    );
    bedGroup.add(
      new THREE.LineSegments(
        axisGeometry,
        new THREE.LineBasicMaterial({
          color: 0x6d8791,
          transparent: true,
          opacity: 0.32,
        }),
      ),
    );
  }

  function frame(center: THREE.Vector3, radius: number, direction: THREE.Vector3) {
    const fov = (camera.fov * Math.PI) / 180;
    const distV = radius / Math.sin(fov / 2);
    const hFov = 2 * Math.atan(Math.tan(fov / 2) * Math.max(camera.aspect, 0.2));
    const distH = radius / Math.sin(hFov / 2);
    const distance = Math.max(distV, distH) * 1.14 + radius * 0.08;
    const dir = direction.clone().normalize();
    controls.target.copy(center);
    camera.position.copy(center).addScaledVector(dir, distance);
    camera.near = Math.max(distance / 10000, 0.1);
    camera.far = distance * 30 + 6000;
    camera.updateProjectionMatrix();
    controls.maxDistance = distance * 12;
    controls.update();
  }

  function view(kind: ViewKind) {
    const box = new THREE.Box3();
    let any = false;
    const explodeFactor = Math.max(explodeProgress, explodeTarget);
    for (const part of currentParts) {
      if (hidden.has(part.id)) continue;
      const offset = entries.get(part.id)?.explode ?? new THREE.Vector3();
      const dx = offset.x * explodeFactor;
      const dy = offset.y * explodeFactor;
      const dz = offset.z * explodeFactor;
      box.expandByPoint(
        new THREE.Vector3(part.min[0] + dx, part.min[1] + dy, part.min[2] + dz),
      );
      box.expandByPoint(
        new THREE.Vector3(part.max[0] + dx, part.max[1] + dy, part.max[2] + dz),
      );
      any = true;
    }
    if (!any) {
      const height = Math.max(bed.z * 0.3, 30);
      box.setFromCenterAndSize(
        new THREE.Vector3(0, 0, Math.min(height, bed.z) / 2),
        new THREE.Vector3(bed.x, bed.y, Math.max(height, 20)),
      );
    }
    const center = box.getCenter(new THREE.Vector3());
    const sphere = box.getBoundingSphere(new THREE.Sphere());
    const radius = Math.max(sphere.radius, 5);
    const directions: Record<Exclude<ViewKind, "fit">, THREE.Vector3> = {
      iso: new THREE.Vector3(0.92, -1.15, 0.8),
      top: new THREE.Vector3(0, -0.14, 1),
      front: new THREE.Vector3(0, -1, 0.16),
      right: new THREE.Vector3(1, 0, 0.16),
    };
    if (kind === "fit") {
      frame(center, radius, directions.iso);
    } else {
      frame(center, radius, directions[kind]);
    }
  }

  const resize = () => {
    const width = host.clientWidth || 1;
    const height = host.clientHeight || 1;
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
  };
  resize();
  const observer = new ResizeObserver(resize);
  observer.observe(host);

  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();
  let downX = 0;
  let downY = 0;

  const onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0) return;
    downX = event.clientX;
    downY = event.clientY;
  };

  const onPointerUp = (event: PointerEvent) => {
    if (event.button !== 0) return;
    if (Math.hypot(event.clientX - downX, event.clientY - downY) > 5) return;
    const rect = renderer.domElement.getBoundingClientRect();
    pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);
    const hits = raycaster.intersectObjects(partsGroup.children, false);
    const hit = hits.find((h) => {
      const id = h.object.userData.partId as number | undefined;
      return id !== undefined && entries.has(id) && h.object.visible;
    });
    const id = hit ? (hit.object.userData.partId as number) : null;
    selectRef.current(id);
  };

  renderer.domElement.addEventListener("pointerdown", onPointerDown);
  renderer.domElement.addEventListener("pointerup", onPointerUp);

  let raf = 0;
  const loop = () => {
    if (disposed) return;
    const dt = Math.min(clock.getDelta(), 0.1);
    const speed = 1 - Math.exp(-dt * 9);
    explodeProgress += (explodeTarget - explodeProgress) * speed;
    if (Math.abs(explodeTarget - explodeProgress) < 1e-4) {
      explodeProgress = explodeTarget;
    }
    const eased = explodeProgress * explodeProgress * (3 - 2 * explodeProgress);
    for (const entry of entries.values()) {
      entry.mesh.position.copy(entry.explode).multiplyScalar(eased);
    }
    controls.update();
    renderer.render(scene, camera);
    raf = requestAnimationFrame(loop);
  };
  raf = requestAnimationFrame(loop);

  buildBed(bed);
  view("fit");

  return {
    setParts,
    setSelected,
    setPlane,
    setBed: buildBed,
    setExploded,
    view,
    dispose: () => {
      disposed = true;
      cancelAnimationFrame(raf);
      observer.disconnect();
      renderer.domElement.removeEventListener("pointerdown", onPointerDown);
      renderer.domElement.removeEventListener("pointerup", onPointerUp);
      controls.dispose();
      for (const entry of entries.values()) {
        entry.mesh.geometry.dispose();
        entry.material.dispose();
      }
      entries.clear();
      disposeObject(bedGroup);
      disposeObject(planeGroup);
      disposeObject(partsGroup);
      renderer.dispose();
      if (renderer.domElement.parentElement === host) {
        host.removeChild(renderer.domElement);
      }
      pending.length = 0;
      onLoading(0);
    },
  };
}

export function Viewport({
  parts,
  bed,
  selectedId,
  hidden,
  plane,
  command,
  exploded,
  onSelect,
  children,
}: ViewportProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const engineRef = useRef<EngineApi | null>(null);
  const selectRef = useRef(onSelect);
  selectRef.current = onSelect;
  const [loading, setLoading] = useState(0);
  const [engineError, setEngineError] = useState(false);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let engine: EngineApi | null = null;
    try {
      engine = createEngine(host, selectRef, setLoading);
      setEngineError(false);
    } catch (error) {
      setEngineError(true);
      console.error("Vue 3D indisponible", error);
    }
    engineRef.current = engine;
    return () => {
      engine?.dispose();
      engineRef.current = null;
    };
  }, []);

  useEffect(() => {
    engineRef.current?.setParts(parts, hidden);
  }, [parts, hidden]);

  useEffect(() => {
    engineRef.current?.setSelected(selectedId);
  }, [selectedId]);

  useEffect(() => {
    engineRef.current?.setPlane(plane);
  }, [plane]);

  useEffect(() => {
    engineRef.current?.setBed(bed);
  }, [bed]);

  useEffect(() => {
    engineRef.current?.setExploded(exploded);
  }, [exploded]);

  useEffect(() => {
    engineRef.current?.view(command.kind);
  }, [command]);

  return (
    <div className="viewport" ref={hostRef}>
      {children}
      {loading > 0 && (
        <div className="viewport-loader">
          <span className="loader-ring" />
          <span>Chargement de l’aperçu…</span>
        </div>
      )}
      {engineError && (
        <div className="viewport-error">
          <strong>Rendu 3D indisponible</strong>
          <span>
            WebGL n’a pas pu démarrer dans cette fenêtre. Vérifiez les pilotes
            graphiques puis relancez l’application.
          </span>
        </div>
      )}
    </div>
  );
}
