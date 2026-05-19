import type { GraphNode, GraphData } from "../types"
import { GRAPH_LAYOUT } from "../constants"

export interface DepthAdjustment {
  nodeId: string
  depthOffset: number // How far forward/back from default z
  zIndex: number // Rendering order
}

export function computeNodeDepths(graph: GraphData): DepthAdjustment[] {
  const adjustments: DepthAdjustment[] = []

  // Sort by relevance score (if available) or importance
  const sortedNodes = [...graph.nodes].sort((a, b) => {
    const aRelevance = a.relevance_score ?? 0
    const bRelevance = b.relevance_score ?? 0
    if (aRelevance !== bRelevance) return bRelevance - aRelevance

    const aImportance = a.importance_score ?? 0.5
    const bImportance = b.importance_score ?? 0.5
    return bImportance - aImportance
  })

  for (let i = 0; i < sortedNodes.length; i++) {
    const node = sortedNodes[i]

    // Primary driver: relevance score
    // Forward depth (toward camera) for high relevance
    let depthOffset = 0
    const relevance = node.relevance_score ?? 0.5

    if (relevance > 0.7) {
      depthOffset = GRAPH_LAYOUT.forwardDepth * (relevance - 0.5) // 0 to forwardDepth
    } else if (relevance < 0.3) {
      depthOffset = -GRAPH_LAYOUT.backwardDepth * (0.5 - relevance) // 0 to -backwardDepth
    }

    // Secondary: importance affects glow/size but not z directly
    // zIndex controls rendering order (higher renders on top)
    const zIndex = Math.round(i)

    adjustments.push({
      nodeId: node.id,
      depthOffset,
      zIndex,
    })
  }

  return adjustments
}

export function applyDepthToNode(
  adjustment: DepthAdjustment,
  baseZ: number
): number {
  // Apply depth offset to base Z position
  return baseZ + adjustment.depthOffset
}

export function getNodeGlowIntensity(node: GraphNode): number {
  // Glow intensity based on importance and relevance
  const importance = node.importance_score ?? 0.5
  const relevance = node.relevance_score ?? 0.5

  // Weighted combination: importance 60%, relevance 40%
  return importance * 0.6 + relevance * 0.4
}

export function getNodeOpacity(node: GraphNode): number {
  // Opacity based on relevance
  const relevance = node.relevance_score ?? 0.5

  // Minimum opacity of 0.3, maximum of 1.0
  return 0.3 + relevance * 0.7
}

export function getNodeSize(node: GraphNode): number {
  // Size primarily based on importance and reuse count
  const importance = node.importance_score ?? 0.5
  const reuse = (node.reuse_count ?? 0) / 10 // Normalize to 0-1
  const confidence = node.confidence_score ?? 0.5

  // Size is weighted average: importance 40%, reuse 40%, confidence 20%
  const baseScore = importance * 0.4 + reuse * 0.4 + confidence * 0.2
  const MIN_SIZE = 0.8
  const MAX_SIZE = 4.0

  return MIN_SIZE + baseScore * (MAX_SIZE - MIN_SIZE)
}
