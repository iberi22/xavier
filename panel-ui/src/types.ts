export interface GraphNode {
  id: string;
  name?: string;
  label?: string;
  type?: string;
  description?: string;
  group?: number;
  val?: number;
  status?: 'active' | 'archived' | 'learning' | string;
  milestone?: string;
  date?: string;
  relatedFiles?: any[];
  decisions?: any[];
  commits?: any[];
  iterations?: any[];
  parentId?: string;
  reason?: string;
}

export interface GraphLink {
  source: string;
  target: string;
  value?: number;
  relation?: string;
}

export interface GraphData {
  nodes: GraphNode[];
  links: GraphLink[];
}

export interface BookmarkArtifact {
  id: string;
  title: string;
  content: string;
  type: 'code' | 'memory' | 'config' | 'log' | string;
  category?: string;
  timestamp: number;
  date?: string;
  tags: string[];
}

export interface CanvasWidget {
  id: string;
  artifact: BookmarkArtifact;
  position: { x: number; y: number };
}

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

export type ThreadDetail = {
  thread: ThreadSummary;
  messages: PanelMessage[];
};

export type PanelChatResponse = {
  thread: ThreadSummary;
  messages: PanelMessage[];
};

export type OnboardingSuggestions = {
  os: string;
  tools: { name: string; installed: boolean; version?: string }[];
  workspace: { project_type: string; indicators: string[] };
  recommendations: string[];
};
