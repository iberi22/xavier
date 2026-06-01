from datetime import datetime
from typing import List, Optional, Dict, Any
from pydantic import BaseModel, Field

class MemoryNode(BaseModel):
    id: str
    path: Optional[str] = None
    content: str
    metadata: Dict[str, Any] = Field(default_factory=dict)
    embedding: Optional[List[float]] = None

class RetrievedMemory(BaseModel):
    id: str
    content: str
    score: float
    source_layer: str
    path: str

class LayerStats(BaseModel):
    working_count: int
    episodic_count: int
    semantic_count: int
    total_results: int

class SearchResponse(BaseModel):
    status: str = "ok"
    count: int = 0
    results: List[MemoryNode]
    query: str
    workspace_id: Optional[str] = None

class RetrieveResponse(BaseModel):
    status: str
    results: List[RetrievedMemory]
    query: str
    layers_used: LayerStats

class GraphNode(BaseModel):
    id: str
    concept: str
    confidence: float
    created_at: datetime

class GraphEdge(BaseModel):
    id: str
    source: str
    target: str
    relation_type: str
    weight: float
    confidence_score: float
    provenance_id: str
    contradicts_edge_id: Optional[str] = None
    created_at: datetime
    updated_at: datetime

class GraphResponse(BaseModel):
    status: str
    nodes: List[GraphNode]
    edges: List[GraphEdge]

class StatsResponse(BaseModel):
    status: str
    workspace_id: str
    version: str
