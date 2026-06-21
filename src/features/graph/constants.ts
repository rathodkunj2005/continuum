import type { Anchor3D } from "./types"

// Layout parameters
export const GRAPH_LAYOUT = {
  communityRadius: 150,
  nodeMinSize: 0.8,
  nodeMaxSize: 4.0,
  forwardDepth: 100,
  backwardDepth: 50,
  zoomMin: 0.1,
  zoomMax: 100,
  defaultFOV: 75,
} as const

// Edge rendering
export const EDGE_CONFIG = {
  maxVisibleEdges: 500,
  topKPerNode: 5,
  defaultOpacity: 0.4,
  edgeWidths: {
    semantic_similarity: 1,
    explicit_reference: 2,
    temporal_adjacency: 1,
    same_project: 1.5,
    same_session: 1.5,
    agent_inferred: 1,
    provenance: 2,
  },
} as const

// Edge colors — cosmic palette, used by overlays + 3D line tinting.
export const EDGE_COLORS: Record<string, string> = {
  semantic_similarity: "#7c5cff", // violet
  explicit_reference: "#5ce0ff", // cyan
  same_project: "#ffc36b", // amber
  temporal_adjacency: "#ff6aa1", // rose
  same_session: "#a8efff", // pale cyan
  agent_inferred: "#b6a4ff", // pale violet
  provenance: "#e8e4fb", // paper
}

// Labels
export const LABEL_CONFIG = {
  maxLabelsVisible: 20,
  defaultFontSize: 12,
  truncateLength: 20,
  topImportanceShown: 5,
} as const

// Community colors (map to Continuum design tokens)
export const COMMUNITY_COLORS: Record<string, string> = {
  "Work/Code": "#5B7FFF", // token-blue
  Research: "#7FFF5B", // token-green
  Design: "#FFD700", // token-gold
  Meetings: "#FFA500", // token-orange
  "Errors/Debugging": "#FF6B6B", // token-red
  People: "#A855F7", // token-purple
  Files: "#06B6D4", // token-cyan
  Decisions: "#BFFF00", // token-lime
  Todos: "#FF69B4", // token-pink
  Concepts: "#6366F1", // token-indigo
  "Past Searches": "#14B8A6", // token-teal
  "Agent Context": "#FCD34D", // token-amber
}

// Community orbital positions (stable, deterministic)
export const CANONICAL_COMMUNITIES: Record<string, Anchor3D> = {
  "Work/Code": { x: -106, y: 106, z: 0 },
  Research: { x: -53, y: 92, z: 92 },
  Design: { x: 53, y: 92, z: 92 },
  Meetings: { x: 106, y: 106, z: 0 },
  "Errors/Debugging": { x: 53, y: 92, z: -92 },
  People: { x: -53, y: 92, z: -92 },
  Files: { x: -106, y: -106, z: 0 },
  Decisions: { x: -53, y: -92, z: -92 },
  Todos: { x: 53, y: -92, z: -92 },
  Concepts: { x: 106, y: -106, z: 0 },
  "Past Searches": { x: 53, y: -92, z: 92 },
  "Agent Context": { x: -53, y: -92, z: 92 },
}

// Animation timings
export const ANIMATION_TIMINGS = {
  nodeTransition: 300, // ms
  cameraFocus: 500, // ms
  labelFade: 200, // ms
  clusterMove: 400, // ms
} as const

// Performance thresholds
export const PERFORMANCE = {
  maxNodesInView: 500,
  aggregateNodeThreshold: 1000,
  largeGraphThreshold: 2000,
  edgeCullDistance: 300,
} as const
