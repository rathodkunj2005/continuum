import type { GraphNode, GraphCommunity, Anchor3D } from "../types"
import { CANONICAL_COMMUNITIES } from "../constants"

export interface NodeLayout {
  nodeId: string
  position: Anchor3D
  x: number
  y: number
  z: number
}

export function computeCommunityAnchors(communities: GraphCommunity[]): GraphCommunity[] {
  // Communities from backend already have anchors; just ensure they're present
  return communities.map((community) => ({
    ...community,
    anchor: community.anchor || getDefaultAnchor(community.label),
  }))
}

export function getDefaultAnchor(communityLabel: string): Anchor3D {
  return CANONICAL_COMMUNITIES[communityLabel] || { x: 0, y: 0, z: 0 }
}

// Deterministic FNV-1a 32-bit hash. Same node id → same offset across renders,
// so layout recomputes don't visibly jitter nodes.
function hashString(s: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = (h * 0x01000193) >>> 0
  }
  return h
}

export function computeLocalNodePositions(
  nodes: GraphNode[],
  communities: GraphCommunity[]
): NodeLayout[] {
  const communityMap = new Map(communities.map((c) => [c.id, c]))
  const layouts: NodeLayout[] = []

  for (const node of nodes) {
    const communityId = node.community_id || "uncategorized"
    const community = communityMap.get(communityId)
    const anchor = community?.anchor || { x: 0, y: 0, z: 0 }

    // Three independent pseudo-random values from one hash, all deterministic.
    const h = hashString(node.id)
    const angle = ((h % 1000) / 1000) * Math.PI * 2 // 0..2π
    const radius = 20 + (((h >>> 10) % 1000) / 1000) * 10 // 20..30
    const yJitter = ((((h >>> 20) % 1000) / 1000) - 0.5) * 10 // -5..+5

    const x = anchor.x + Math.cos(angle) * radius
    const y = anchor.y + yJitter
    const z = anchor.z + Math.sin(angle) * radius

    layouts.push({
      nodeId: node.id,
      position: { x, y, z },
      x,
      y,
      z,
    })
  }

  return layouts
}

// Simple force-directed layout for nodes within a community
export function simulateLocalForces(
  layouts: NodeLayout[],
  edges: { source: string; target: string; weight: number }[],
  iterations: number = 20
): NodeLayout[] {
  const result = layouts.map((l) => ({ ...l }))

  const nodeMap = new Map(result.map((l) => [l.nodeId, l]))

  for (let iter = 0; iter < iterations; iter++) {
    for (const layout of result) {
      let fx = 0
      let fy = 0
      let fz = 0

      // Repulsion from other nodes
      for (const other of result) {
        if (layout.nodeId === other.nodeId) continue

        const dx = other.x - layout.x
        const dy = other.y - layout.y
        const dz = other.z - layout.z
        const dist = Math.sqrt(dx * dx + dy * dy + dz * dz) + 0.1 // Avoid zero division

        // Repulsive force (inverse square law)
        const repulsion = 100 / (dist * dist)
        fx -= (dx / dist) * repulsion
        fy -= (dy / dist) * repulsion
        fz -= (dz / dist) * repulsion
      }

      // Attraction from edges
      for (const edge of edges) {
        if (edge.source === layout.nodeId) {
          const target = nodeMap.get(edge.target)
          if (target) {
            const dx = target.x - layout.x
            const dy = target.y - layout.y
            const dz = target.z - layout.z
            const dist = Math.sqrt(dx * dx + dy * dy + dz * dz) + 0.1

            const attraction = edge.weight * 0.5
            fx += (dx / dist) * attraction
            fy += (dy / dist) * attraction
            fz += (dz / dist) * attraction
          }
        }
      }

      // Apply forces (damped)
      const damping = 0.9
      layout.x += fx * 0.01 * damping
      layout.y += fy * 0.01 * damping
      layout.z += fz * 0.01 * damping
    }
  }

  return result
}
