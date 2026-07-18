export type GraphLayer = "roadmap" | "memory" | "code";

export interface CanvasNode {
  id: string;
  label: string;
  kind: string;
  description?: string;
  meta?: Record<string, unknown>;
}

export interface CanvasLink {
  source: string;
  target: string;
  relation: string;
  weight?: number;
}

export interface CanvasGraph {
  layer: GraphLayer;
  nodes: CanvasNode[];
  links: CanvasLink[];
  truncated?: boolean;
  stats?: Record<string, number | string>;
}
