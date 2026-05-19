import React, { useMemo } from "react"
import type { GraphNode, GraphCommunity } from "../types"
import { useGraphStore } from "../state/graphStore"
import { COMMUNITY_COLORS } from "../constants"
import { getNodeDisplayTitle } from "../utils/displayTitle"

interface NodeLayout {
  nodeId: string
  position: { x: number; y: number; z: number }
  x: number
  y: number
  z: number
}

interface Label {
  id: string
  text: string
  position: { x: number; y: number; z: number }
  color: string
  isSelected: boolean
  isCommunity: boolean
}

interface GraphLabelsProps {
  graphData: { nodes: GraphNode[] }
  nodePositions: NodeLayout[]
  communities: GraphCommunity[]
}

export const GraphLabels: React.FC<GraphLabelsProps> = ({
  graphData,
  nodePositions,
  communities,
}) => {
  const selectedNodeId = useGraphStore((s) => s.selectedNodeId)
  const hoveredNodeId = useGraphStore((s) => s.hoveredNodeId)
  const showLabels = useGraphStore((s) => s.showLabels)

  if (!showLabels) return null

  // Compute labels with strict discipline — very sparse
  const labels = useMemo(() => {
    const result: Label[] = []
    const nodeMap = new Map(graphData.nodes.map((n) => [n.id, n]))
    const maxLabels = 10 // Very aggressive cap

    // 1. Community labels (always shown, up to 5)
    communities.slice(0, 5).forEach((community) => {
      result.push({
        id: `community-${community.id}`,
        text: community.label,
        position: community.anchor,
        color: COMMUNITY_COLORS[community.label] || "#D4AF37", // Muted gold default
        isSelected: false,
        isCommunity: true,
      })
    })

    // 2. Selected node label
    if (selectedNodeId && result.length < maxLabels) {
      const selectedNode = nodeMap.get(selectedNodeId)
      const selectedPos = nodePositions.find((p) => p.nodeId === selectedNodeId)
      if (selectedNode && selectedPos) {
        const displayTitle = getNodeDisplayTitle(selectedNode)
        result.push({
          id: `node-${selectedNodeId}`,
          text: displayTitle.length > 32 ? displayTitle.substring(0, 32) + "…" : displayTitle,
          position: selectedPos.position,
          color: "#F5F5DC", // Off-white
          isSelected: true,
          isCommunity: false,
        })
      }
    }

    // 3. Hovered node label (only if different from selected)
    if (hoveredNodeId && hoveredNodeId !== selectedNodeId && result.length < maxLabels) {
      const hoveredNode = nodeMap.get(hoveredNodeId)
      const hoveredPos = nodePositions.find((p) => p.nodeId === hoveredNodeId)
      if (hoveredNode && hoveredPos) {
        const displayTitle = getNodeDisplayTitle(hoveredNode)
        result.push({
          id: `node-${hoveredNodeId}`,
          text: displayTitle.length > 32 ? displayTitle.substring(0, 32) + "…" : displayTitle,
          position: hoveredPos.position,
          color: "#FFD700", // Bright gold for hover
          isSelected: false,
          isCommunity: false,
        })
      }
    }

    // 4. Very few top important nodes (max 2-3 total, already have community + selected)
    const importantNodes = graphData.nodes
      .filter(
        (n) =>
          n.id !== selectedNodeId &&
          n.id !== hoveredNodeId &&
          n.importance_score &&
          n.importance_score > 0.8
      )
      .sort((a, b) => (b.importance_score ?? 0) - (a.importance_score ?? 0))
      .slice(0, Math.max(0, maxLabels - result.length - 1))

    importantNodes.forEach((node) => {
      const pos = nodePositions.find((p) => p.nodeId === node.id)
      if (pos && result.length < maxLabels) {
        const displayTitle = getNodeDisplayTitle(node)
        result.push({
          id: `node-${node.id}`,
          text: displayTitle.length > 28 ? displayTitle.substring(0, 28) + "…" : displayTitle,
          position: pos.position,
          color: "#B8B8B8", // Dim off-white
          isSelected: false,
          isCommunity: false,
        })
      }
    })

    return result.slice(0, maxLabels)
  }, [selectedNodeId, hoveredNodeId, nodePositions, communities, graphData.nodes])

  return (
    <div className="absolute inset-0 pointer-events-none">
      {labels.map((label) => (
        <div
          key={label.id}
          style={{
            position: "absolute",
            left: "50%",
            top: "50%",
            pointerEvents: "none",
            transform: "translate(-50%, -50%)",
            zIndex: label.isSelected ? 100 : label.isCommunity ? 60 : 50,
          }}
          className="whitespace-nowrap"
        >
          <div
            style={{
              padding: label.isCommunity ? "4px 8px" : "2px 6px",
              borderRadius: "4px",
              fontSize: label.isCommunity ? "12px" : "11px",
              fontWeight: label.isCommunity ? "600" : "500",
              color: label.color,
              backgroundColor: "rgba(10, 14, 39, 0.92)",
              border: `1px solid ${label.color}40`,
              maxWidth: "140px",
              overflow: "hidden",
              textOverflow: "ellipsis",
              opacity: label.isSelected ? 1 : 0.8,
              transition: "opacity 150ms",
              fontFamily: "system-ui, -apple-system, sans-serif",
            }}
          >
            {label.text}
          </div>
        </div>
      ))}
    </div>
  )
}
