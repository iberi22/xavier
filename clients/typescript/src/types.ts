export interface MemoryNode {
  id: string;
  path: string;
  content: string;
  metadata: Record<string, any>;
}

export interface SearchResponse {
  status: string;
  results: MemoryNode[];
  query: string;
  count: number;
  workspace_id?: string;
}

export interface RetrievedMemory {
  id: string;
  content: string;
  score: number;
  source_layer: string;
  path: string;
}

export interface LayerStats {
  working_count: number;
  episodic_count: number;
  semantic_count: number;
  total_results: number;
}

export interface RetrieveResponse {
  status: string;
  results: RetrievedMemory[];
  query: string;
  layers_used: LayerStats;
}

export interface StatsResponse {
  status: string;
  workspace_id: string;
  version: string;
}

export interface AddMemoryRequest {
  content: string;
  path?: string;
  metadata?: Record<string, any>;
  [key: string]: any;
}

export interface DeleteResponse {
  status: string;
  deleted: boolean;
  id?: string;
  path?: string;
}

export interface ClientOptions {
  baseUrl?: string;
  token?: string;
}
