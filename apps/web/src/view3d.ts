// 3D viewport (three.js, WebGL2). Flat-shaded faces in style colors, feature edges as constant-width lines
// (never derived from triangles), own tiny orbit/pan/zoom controller, orthographic camera (plotter look).
import {
  WebGLRenderer, Scene, OrthographicCamera, BufferGeometry, Float32BufferAttribute, Mesh as TMesh,
  MeshLambertMaterial, Color, AmbientLight, DirectionalLight, GridHelper, Vector3, Box3, Raycaster, Vector2,
  Matrix4, LineBasicMaterial, LineSegments, Object3D, Plane, PlaneGeometry, MeshBasicMaterial, BackSide, FrontSide,
  IncrementWrapStencilOp, DecrementWrapStencilOp, type StencilOp, AlwaysStencilFunc, NotEqualStencilFunc, ReplaceStencilOp,
} from 'three';
import { LineSegments2 } from 'three/addons/lines/LineSegments2.js';
import { LineSegmentsGeometry } from 'three/addons/lines/LineSegmentsGeometry.js';
import { LineMaterial } from 'three/addons/lines/LineMaterial.js';
import type { Mesh } from './types';

export type Preset = 'front' | 'iso' | 'top' | 'right';
const PAPER = 0xf2efe6, INK = 0x1a1a1a, GRID2 = 0xd3e0ee, GRID = 0xa9c1dd, BLUE = 0x1d4e9e, MANILA = 0xe9d9a6;

interface PartObj { src: string; mesh: TMesh; edges: LineSegments2 | LineSegments; mat: MeshLambertMaterial }

export class View3D {
  readonly canvas: HTMLCanvasElement;
  private renderer!: WebGLRenderer;
  private scene = new Scene();
  private camera = new OrthographicCamera(-1, 1, 1, -1, -10000, 10000);
  private target = new Vector3();
  private az = Math.PI / 4; private el = Math.PI / 6; // iso defaults
  private radius = 100; // ortho: only scales the near/far; zoom via camera.zoom
  private parts: PartObj[] = [];
  private group = new Object3D();
  private grid: GridHelper | null = null;
  private box = new Box3();
  private raf = 0;
  private w = 1; private h = 1;
  private selected: string | null = null;
  private hover: string | null = null;
  private lineMats: LineMaterial[] = [];
  private ray = new Raycaster();
  private drag: null | { x: number; y: number; btn: number; moved: boolean } = null;
  failed: string | null = null;
  fovFit = 1;

  constructor(private host: HTMLElement, private cb: { onSelect(id: string | null): void; onHover(id: string | null): void }) {
    this.canvas = document.createElement('canvas');
    this.canvas.className = 'vp-canvas';
    host.appendChild(this.canvas);
    try {
      this.renderer = new WebGLRenderer({ canvas: this.canvas, antialias: true, alpha: false, stencil: true, powerPreference: 'high-performance' });
    } catch (e) {
      this.failed = 'WEBGL2 NOT AVAILABLE (' + (e as Error).message + ')';
      return;
    }
    this.renderer.setClearColor(PAPER, 1);
    this.renderer.localClippingEnabled = true;
    this.scene.add(new AmbientLight(0xffffff, 1.15));
    const key = new DirectionalLight(0xffffff, 1.35); key.position.set(-0.5, 1.2, 1.0); this.scene.add(key);
    const fill = new DirectionalLight(0xffffff, 0.35); fill.position.set(1, -0.3, 0.2); this.scene.add(fill);
    this.scene.add(this.group);
    new ResizeObserver(() => this.resize()).observe(host);
    this.canvas.addEventListener('wheel', (e) => { e.preventDefault(); this.zoomBy(Math.exp(-(e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY) * 0.0016)); }, { passive: false });
    this.canvas.addEventListener('pointerdown', (e) => { this.canvas.setPointerCapture(e.pointerId); this.drag = { x: e.clientX, y: e.clientY, btn: e.button, moved: false }; });
    this.canvas.addEventListener('pointermove', (e) => this.onMove(e));
    this.canvas.addEventListener('pointerup', (e) => this.onUp(e));
    this.canvas.addEventListener('dblclick', () => this.fit());
    this.resize();
  }

  get ok() { return !this.failed; }

  setMesh(mesh: Mesh | null) {
    if (this.failed) return;
    for (const p of this.parts) { p.mesh.geometry.dispose(); p.mat.dispose(); p.edges.geometry.dispose(); }
    this.group.clear();
    this.parts = [];
    this.lineMats = [];
    if (this.grid) { this.scene.remove(this.grid); this.grid.dispose(); this.grid = null; }
    this.box.makeEmpty();
    if (!mesh) { this.requestRender(); return; }
    const lineMat = new LineMaterial({ color: INK, linewidth: 1.25, worldUnits: false });
    this.lineMats.push(lineMat);
    for (const part of mesh.parts) {
      const g = new BufferGeometry();
      g.setAttribute('position', new Float32BufferAttribute(part.positions, 3));
      if (part.normals?.length === part.positions.length) g.setAttribute('normal', new Float32BufferAttribute(part.normals, 3));
      g.setIndex(part.indices);
      if (!part.normals?.length) g.computeVertexNormals();
      const col = new Color(part.color || '#C9A46A');
      // muted, slightly desaturated drafting tone
      const hsl = { h: 0, s: 0, l: 0 }; col.getHSL(hsl); col.setHSL(hsl.h, hsl.s * 0.82, Math.min(0.9, hsl.l * 1.02 + 0.02));
      const mat = new MeshLambertMaterial({ color: col, flatShading: true, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1 });
      const mesh3 = new TMesh(g, mat);
      mesh3.userData.src = part.src;
      this.group.add(mesh3);
      let edges: LineSegments2 | LineSegments;
      if (part.edges.length >= 6) {
        const lg = new LineSegmentsGeometry();
        lg.setPositions(part.edges);
        edges = new LineSegments2(lg, lineMat);
      } else {
        edges = new LineSegments(new BufferGeometry(), new LineBasicMaterial({ color: INK }));
      }
      this.group.add(edges);
      this.parts.push({ src: part.src, mesh: mesh3, edges, mat });
      g.computeBoundingBox();
      if (g.boundingBox) this.box.union(g.boundingBox);
    }
    if (!this.box.isEmpty()) {
      const size = this.box.getSize(new Vector3());
      const span = Math.max(size.x, size.y, size.z, 12);
      const divs = Math.min(200, Math.max(10, Math.ceil((span * 2.2) / 12)));
      const gsize = divs * 12;
      this.grid = new GridHelper(gsize, divs, GRID, GRID2);
      const c = this.box.getCenter(new Vector3());
      this.grid.position.set(Math.round(c.x / 12) * 12, this.box.min.y - 0.01, Math.round(c.z / 12) * 12);
      this.scene.add(this.grid);
    }
    this.applyHighlight();
    if (this.cutZ !== null) this.setCut(this.cutZ);
    this.fit();
  }

  private cutPlane = new Plane(new Vector3(0, 0, -1), 0);
  private cutZ: number | null = null;
  private capObjs: Object3D[] = [];

  /** Section cut at z (keeps z <= cutZ, the far side from the viewer) with manila stencil caps; null = off. */
  setCut(z: number | null) {
    this.cutZ = z;
    for (const o of this.capObjs) { this.group.remove(o); (o as TMesh).geometry?.dispose(); }
    this.capObjs = [];
    const on = z !== null;
    this.cutPlane.constant = z ?? 0;
    for (const p of this.parts) {
      const planes = on ? [this.cutPlane] : [];
      p.mat.clippingPlanes = planes; p.mat.needsUpdate = true;
      if (p.edges instanceof LineSegments2) { p.edges.material.clippingPlanes = planes; p.edges.material.needsUpdate = true; }
    }
    if (on && !this.box.isEmpty()) {
      const size = this.box.getSize(new Vector3()), c = this.box.getCenter(new Vector3());
      const span = Math.max(size.x, size.y) * 1.2 + 10;
      let order = 1;
      for (const p of this.parts) {
        const mk = (side: typeof BackSide | typeof FrontSide, op: StencilOp) => {
          const m = new MeshBasicMaterial({ side, depthWrite: false, depthTest: false, colorWrite: false, stencilWrite: true, stencilFunc: AlwaysStencilFunc, clippingPlanes: [this.cutPlane] });
          m.stencilFail = op; m.stencilZFail = op; m.stencilZPass = op;
          const o = new TMesh(p.mesh.geometry, m);
          o.renderOrder = order;
          return o;
        };
        const back = mk(BackSide, IncrementWrapStencilOp), front = mk(FrontSide, DecrementWrapStencilOp);
        const capMat = new MeshBasicMaterial({ color: MANILA, stencilWrite: true, stencilRef: 0, stencilFunc: NotEqualStencilFunc, stencilFail: ReplaceStencilOp, stencilZFail: ReplaceStencilOp, stencilZPass: ReplaceStencilOp });
        const cap = new TMesh(new PlaneGeometry(span, span), capMat);
        cap.position.set(c.x, c.y, z!);
        cap.renderOrder = order + 0.1;
        cap.onAfterRender = (r) => r.clearStencil();
        this.group.add(back, front, cap);
        this.capObjs.push(back, front, cap);
        order += 1;
      }
    }
    this.requestRender();
  }
  get cut() { return this.cutZ; }

  setSelection(id: string | null) { this.selected = id; this.applyHighlight(); this.requestRender(); }
  setHover(id: string | null) { this.hover = id; this.applyHighlight(); this.requestRender(); }

  private applyHighlight() {
    const base = (s: string) => { const i = s.search(/[.#]/); return i < 0 ? s : s.slice(0, i); };
    for (const p of this.parts) {
      const b = base(p.src);
      p.mat.emissive.setHex(b === this.selected ? BLUE : b === this.hover ? 0x16346a : 0x000000);
      p.mat.emissiveIntensity = b === this.selected ? 0.42 : b === this.hover ? 0.2 : 0;
    }
  }

  preset(p: Preset) {
    const map: Record<Preset, [number, number]> = {
      front: [0, 0],
      iso: [Math.PI / 4, Math.asin(Math.tan(Math.PI / 6))],
      top: [0, Math.PI / 2 - 1e-3],
      right: [Math.PI / 2, 0],
    };
    [this.az, this.el] = map[p];
    this.fit();
  }

  private camDir(): Vector3 {
    // camera sits at +Z (front), azimuth rotates toward +X, elevation up
    return new Vector3(Math.sin(this.az) * Math.cos(this.el), Math.sin(this.el), Math.cos(this.az) * Math.cos(this.el));
  }

  private updateCamera() {
    const dir = this.camDir();
    this.camera.position.copy(this.target).addScaledVector(dir, this.radius);
    this.camera.up.set(0, 1, 0);
    this.camera.lookAt(this.target);
    this.camera.updateMatrixWorld();
    const a = this.w / this.h;
    const half = this.viewHalf;
    this.camera.left = -half * a; this.camera.right = half * a; this.camera.top = half; this.camera.bottom = -half;
    this.camera.near = -this.radius * 20; this.camera.far = this.radius * 20;
    this.camera.updateProjectionMatrix();
  }
  private viewHalf = 50;

  fit() {
    if (this.failed) return;
    if (this.box.isEmpty()) { this.requestRender(); return; }
    this.box.getCenter(this.target);
    const size = this.box.getSize(new Vector3());
    this.radius = Math.max(size.length() * 2, 100);
    this.updateCamera();
    // project the 8 box corners into camera space
    const inv = new Matrix4().copy(this.camera.matrixWorld).invert();
    let mx = 0, my = 0;
    for (let i = 0; i < 8; i++) {
      const v = new Vector3(i & 1 ? this.box.max.x : this.box.min.x, i & 2 ? this.box.max.y : this.box.min.y, i & 4 ? this.box.max.z : this.box.min.z).applyMatrix4(inv);
      mx = Math.max(mx, Math.abs(v.x - 0)); my = Math.max(my, Math.abs(v.y - 0));
    }
    // v is relative to camera; center offset handled because target is the box center (projects to origin)
    const a = this.w / this.h;
    this.viewHalf = Math.max(my * 1.18, (mx * 1.18) / a, 4);
    this.camera.zoom = 1;
    this.updateCamera();
    this.requestRender();
  }

  zoomBy(f: number) {
    this.camera.zoom = Math.max(0.05, Math.min(this.camera.zoom * f, 200));
    this.camera.updateProjectionMatrix();
    this.requestRender();
  }

  resize() {
    if (this.failed) return;
    const r = this.host.getBoundingClientRect();
    const w = Math.max(1, Math.round(r.width)), h = Math.max(1, Math.round(r.height));
    this.w = w; this.h = h;
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setSize(w, h, false);
    this.canvas.style.width = w + 'px'; this.canvas.style.height = h + 'px';
    for (const m of this.lineMats) m.resolution.set(w * this.renderer.getPixelRatio(), h * this.renderer.getPixelRatio());
    this.updateCamera();
    this.requestRender();
  }

  private onMove(e: PointerEvent) {
    const d = this.drag;
    if (!d) {
      const src = this.pickAt(e);
      if (src !== this.hover) { this.hover = src; this.cb.onHover(src); this.applyHighlight(); this.requestRender(); }
      return;
    }
    const dx = e.clientX - d.x, dy = e.clientY - d.y;
    if (!d.moved && Math.hypot(dx, dy) < 3) return;
    d.moved = true;
    d.x = e.clientX; d.y = e.clientY;
    if (d.btn === 0 && !e.shiftKey) {
      this.az -= dx * 0.008;
      this.el = Math.max(-1.5, Math.min(1.5, this.el + dy * 0.008));
      this.updateCamera();
    } else {
      // pan in the camera plane
      const unit = (this.viewHalf * 2) / (this.h * this.camera.zoom);
      const right = new Vector3().setFromMatrixColumn(this.camera.matrixWorld, 0);
      const up = new Vector3().setFromMatrixColumn(this.camera.matrixWorld, 1);
      this.target.addScaledVector(right, -dx * unit).addScaledVector(up, dy * unit);
      this.updateCamera();
    }
    this.requestRender();
  }

  private onUp(e: PointerEvent) {
    const d = this.drag;
    this.drag = null;
    if (d && !d.moved && d.btn === 0) this.cb.onSelect(this.pickAt(e));
  }

  private pickAt(e: PointerEvent): string | null {
    const r = this.canvas.getBoundingClientRect();
    const nx = ((e.clientX - r.left) / r.width) * 2 - 1, ny = -((e.clientY - r.top) / r.height) * 2 + 1;
    this.ray.setFromCamera(new Vector2(nx, ny), this.camera);
    const hits = this.ray.intersectObjects(this.parts.map((p) => p.mesh), false);
    const src = hits[0]?.object.userData.src as string | undefined;
    if (!src) return null;
    const i = src.search(/[.#]/);
    return i < 0 ? src : src.slice(0, i);
  }

  requestRender() {
    if (this.raf || this.failed) return;
    this.raf = requestAnimationFrame(() => { this.raf = 0; this.renderer.render(this.scene, this.camera); });
  }

  /** Synchronous render, e.g. before taking a screenshot. */
  renderNow() { if (!this.failed) this.renderer.render(this.scene, this.camera); }

  dispose() { this.renderer?.dispose(); }
}
