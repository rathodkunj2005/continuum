import React, { useMemo } from "react"
import * as THREE from "three"
import type { GraphData, EdgeType } from "../types"
import { useGraphStore } from "../state/graphStore"
import { selectVisibleEdges, getEdgeColor, getEdgeWidth } from "../layout/edgeVisibility"

interface NodeLayout {
  nodeId: string
  position: { x: number; y: number; z: number }
  x: number
  y: number
  z: number
}

interface GraphEdgesProps {
  graphData: GraphData
  nodePositions: NodeLayout[]
}

function EdgeLine({
  source,
  target,
  edgeType,
  weight,
  confidence,
}: {
  source: { x: number; y: number; z: number }
  target: { x: number; y: number; z: number }
  edgeType: EdgeType
  weight: number
  confidence?: number
}) {
  const color = useMemo(() => getEdgeColor(edgeType), [edgeType])
  const width = useMemo(() => getEdgeWidth(edgeType, weight), [edgeType, weight])
  const opacity = useMemo(() => (confidence ?? weight) * 0.6, [confidence, weight])

  const geometry = useMemo(() => {
    const geometry = new THREE.BufferGeometry()
    const positions = new Float32Array([source.x, source.y, source.z, target.x, target.y, target.z])
    geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3))
    return geometry
  }, [source, target])

  const material = useMemo(() => {
    return new THREE.LineBasicMaterial({
      color,
      opacity: Math.min(opacity, 1),
      transparent: true,
      linewidth: width,
      fog: true,
    })
  }, [color, opacity, width])

  return <lineSegments geometry={geometry} material={material} />
}

export const GraphEdges: React.FC<GraphEdgesProps> = ({ graphData, nodePositions }) => {
  const selectedNodeId = useGraphStore((s) => s.selectedNodeId)
  const enabledEdgeTypes = useGraphStore((s) => s.enabledEdgeTypes)

  // Create position map for fast lookup
  const nodePositionMap = useMemo(() => {
    return new Map(nodePositions.map((p) => [p.nodeId, p.position]))
  }, [nodePositions])

  // Select visible edges with very conservative filtering
  const visibleEdges = useMemo(() => {
    // Only show edges for selected/hovered nodes to avoid visual noise
    const selectedSet = new Set<string>()
    if (selectedNodeId) selectedSet.add(selectedNodeId)

    // Very sparse default: only show selected node edges, max 8 total
    const maxEdges = selectedNodeId ? 8 : 0
    return selectVisibleEdges(graphData.edges, selectedSet, enabledEdgeTypes, maxEdges)
  }, [graphData.edges, selectedNodeId, enabledEdgeTypes])

  // Filter to only edges with valid positions
  const renderedEdges = useMemo(() => {
    return visibleEdges.filter((edge) => {
      const sourcePos = nodePositionMap.get(edge.source)
      const targetPos = nodePositionMap.get(edge.target)
      return sourcePos && targetPos
    })
  }, [visibleEdges, nodePositionMap])

  return (
    <group>
      {renderedEdges.map((edge) => {
        const sourcePos = nodePositionMap.get(edge.source)
        const targetPos = nodePositionMap.get(edge.target)

        if (!sourcePos || !targetPos) return null

        return (
          <EdgeLine
            key={edge.id}
            source={sourcePos}
            target={targetPos}
            edgeType={edge.edge_type}
            weight={edge.weight}
            confidence={edge.confidence}
          />
        )
      })}
    </group>
  )
}
