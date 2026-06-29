export type ThreadSummary = {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  last_preview: string;
  message_count: number;
};

export type PanelMessage = {
  id: string;
  role: string;
  plain_text: string;
  openui_lang?: string | null;
  created_at: string;
  metadata?: {
    confidence?: number;
    timings?: {
      system1_ms: number;
      system2_ms: number;
      system3_ms: number;
      total_ms: number;
    };
    components?: string[];
    rules?: string[];
    documents?: number;
    evidence?: number;
  };
};

export type Bookmark = {
  id: string;
  title: string;
  url: string;
  metadata: Record<string, unknown>;
  created_at: string;
};

export type Widget = {
  id: string;
  type: string;
  config: Record<string, unknown>;
  x: number;
  y: number;
  w: number;
  h: number;
  created_at: string;
};

export interface GraphNode {
  id: string;
  label: string;
  type: "organization" | "project" | "subproject" | "session";
  description: string;
  parentId?: string;
  date?: string;
  milestone?: string;
  reason?: string;
  relatedFiles?: string[];
  decisions?: string[];
  commits?: string[];
  iterations?: string[];
}

export interface GraphLink {
  source: string;
  target: string;
  relation: string;
}

export interface GraphData {
  nodes: GraphNode[];
  links: GraphLink[];
}

export type BackendGraphData = {
  id: string;
  name: string;
  data: GraphData;
  created_at: string;
};

export type OnboardingSuggestions = {
  os: string;
  tools: { name: string; installed: boolean; version?: string }[];
  workspace: { project_type: string; indicators: string[] };
  recommendations: string[];
};

export type ThreadDetail = {
  thread: ThreadSummary;
  messages: PanelMessage[];
};

export type PanelChatResponse = {
  thread: ThreadSummary;
  messages: PanelMessage[];
};

export interface BookmarkArtifact {
  id: string;
  title: string;
  description?: string;
  type: "file" | "memory" | "document" | "agent" | string;
  url?: string;
  addedAt?: string;
  date?: string;
  category?: string;
}

export interface CanvasWidget {
  id: string;
  artifact: BookmarkArtifact;
  position: { x: number; y: number };
}

export interface MemoryEntry {
  id: string;
  content: string;
  kind: string;
  priority: string;
  source: string;
  created_at: string;
}

export interface User {
  id: string;
  email: string;
  name: string;
  role: "admin" | "user" | "readonly";
  api_key?: string;
  created_at: number;
  updated_at: number;
}

export interface AuthState {
  user: User | null;
  token: string | null;
  refreshToken: string | null;
  isAuthenticated: boolean;
  requires2FA: boolean;

  login: (email: string, password: string, totpCode?: string) => Promise<void>;
  logout: () => Promise<void>;
  register: (email: string, name: string, password: string) => Promise<any>;
  refreshSession: () => Promise<void>;
  checkUsers?: () => Promise<{ has_users: boolean; count: number }>;
}

export interface Agent {
  id: string;
  name: string;
  status: "running" | "stopped" | "error";
  last_seen: string;
  metadata?: Record<string, unknown>;
}

export type MeshRole = "admin" | "editor" | "reader";

export type ClearanceLevel =
  | "unclassified"
  | "confidential"
  | "secret"
  | "top_secret";

export interface MeshPeer {
  node_id: string;
  alias?: string;
  endpoint_url: string;
  role: MeshRole;
  clearance: ClearanceLevel;
  last_seen_at: number | null;
  sync_enabled: boolean;
}

export interface MeshStatus {
  peers: MeshPeer[];
  local_node_id: string;
}

export interface PairingCodeResponse {
  code: string;
  secret: string;
}
