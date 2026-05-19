export enum NodeType {
  Memory = "memory",
  Entity = "entity",
  Community = "community",
  Evidence = "evidence",
  AgentContext = "agent_context",
}

export enum EdgeType {
  SemanticSimilarity = "semantic_similarity",
  ExplicitReference = "explicit_reference",
  TemporalAdjacency = "temporal_adjacency",
  SameProject = "same_project",
  SameSession = "same_session",
  AgentInferred = "agent_inferred",
  Provenance = "provenance",
}

export interface GraphNode {
  id: string
  node_type: NodeType
  title: string
  summary?: string
  community_id?: string
  timestamp_start?: string
  timestamp_end?: string
  app_name?: string
  window_title?: string
  url?: string
  project?: string
  topic?: string
  activity_type?: string
  importance_score?: number
  relevance_score?: number
  confidence_score?: number
  reuse_count?: number
  source_ids?: string[]
  metadata?: Record<string, unknown>
}

export interface GraphEdge {
  id: string
  source: string
  target: string
  edge_type: EdgeType
  weight: number
  confidence?: number
  reason?: string
  metadata?: Record<string, unknown>
}

export interface Anchor3D {
  x: number
  y: number
  z: number
}

export interface GraphCommunity {
  id: string
  label: string
  description?: string
  color_token?: string
  anchor: Anchor3D
  node_count?: number
  importance_score?: number
}

export enum FocusType {
  Query = "query",
  Project = "project",
  Memory = "memory",
  AgentTask = "agent_task",
  Atlas = "atlas",
}

export interface ActiveFocus {
  focus_type: FocusType
  id?: string
  label: string
  query?: string
}

export interface GraphData {
  nodes: GraphNode[]
  edges: GraphEdge[]
  communities: GraphCommunity[]
  active_focus?: ActiveFocus
}

// Internal graph state
export interface GraphUIState {
  mode: "context" | "atlas"
  selectedNodeId: string | null
  hoveredNodeId: string | null
  expandedNodeIds: Set<string>
  selectedCommunityIds: Set<string>
  enabledNodeTypes: Set<NodeType>
  enabledEdgeTypes: Set<EdgeType>
  zoomLevel: number
  isLoading: boolean
  error: string | null
  showEvidence: boolean
  showLabels: boolean
}
