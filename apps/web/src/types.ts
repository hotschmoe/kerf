// Shared types: Kerf document, Drawing IR (SPEC §10), Mesh (SPEC §11), engine API (SPEC §13), ops (SPEC §14).

export type Pt = [number, number];
export type BPt = [number, number] | [number, number, number]; // x, y, bulge?

export interface Citation {
  code: string;
  edition?: number;
  section?: string;
  title?: string;
  status: 'suggested' | 'verified';
}

export interface Annotation {
  id: string;
  type: 'note' | 'dim' | 'label' | string;
  text?: string | null;
  target?: string;
  at?: unknown;
  place?: Pt | null;
  cite?: Citation[];
  [k: string]: unknown;
}

export interface View {
  id: string;
  kind: 'section' | 'iso' | string;
  number?: string;
  title?: string;
  scale?: string;
  cut_z?: number;
  crop?: { x: Pt; y: Pt };
  annotations?: Annotation[];
  [k: string]: unknown;
}

export interface Component {
  id: string;
  type: string;
  label?: string;
  [k: string]: unknown;
}

export interface KerfDoc {
  kerf: string;
  id: string;
  title?: string;
  meta?: Record<string, unknown>;
  run?: Pt;
  components: Component[];
  views: View[];
  [k: string]: unknown;
}

export interface Pen {
  width_mm: number;
  dash_mm?: number[] | null;
}

export type DrawItem =
  | { t: 'path'; layer?: string; pen: string; src?: string; closed?: boolean; pts: BPt[] }
  | { t: 'fill'; layer?: string; src?: string; loops: BPt[][] }
  | { t: 'hatch'; layer?: string; pen?: string; src?: string; pattern?: string; scale?: number; angle?: number; loops: BPt[][]; lines: number[][] }
  | { t: 'text'; layer?: string; pen?: string; src?: string; s: string; x: number; y: number; h: number; rot?: number; align?: string; valign?: string };

export interface Diagnostic {
  level: 'error' | 'warning' | 'info';
  code: string;
  id?: string;
  path?: string;
  message: string;
  fix?: string;
}

export interface Drawing {
  kerf_drawing: string;
  doc: string;
  view: string;
  kind: string;
  scale: number;
  bounds: [number, number, number, number];
  pens: Record<string, Pen>;
  layers?: { name: string; lineweight_mm?: number }[];
  items: DrawItem[];
  diagnostics?: Diagnostic[];
}

export interface MeshPart {
  src: string;
  part: string | null;
  instance: number;
  material: string;
  color: string;
  positions: number[];
  normals: number[];
  indices: number[];
  edges: number[];
}
export interface Mesh { kerf_mesh: string; parts: MeshPart[] }

export interface Op {
  op: 'add' | 'update' | 'remove' | 'set';
  path: string;
  value?: unknown;
  before?: string;
}

export interface ApplyResult {
  ok: boolean;
  doc?: KerfDoc;
  diagnostics: Diagnostic[];
  summary: string;
  changed?: string[];
  error?: string;
  [k: string]: unknown;
}

export interface CheckResult { diagnostics: Diagnostic[]; summary: string }

export type Actor = 'llm' | 'designer';

export interface StrokeFont {
  name: string;
  cap_height: number;
  baseline: number;
  glyphs: Record<string, { adv: number; strokes: number[][][] }>;
}
