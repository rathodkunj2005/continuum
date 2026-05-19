import React, { useMemo, useCallback } from "react"
import * as THREE from "three"
import type { GraphData } from "../types"
import { useGraphStore } from "../state/graphStore"
import { getNodeGeometry } from "../rendering/geometries"
import { createNodeMaterial, createGlowMaterial } from "../rendering/materials"
import {
  getNodeSize,
  getNodeGlowIntensity,
  getNodeOpacity,
  computeNodeDepths,
} from "../layout/depthComputation"
import { COMMUNITY_COLORS } from "../constants"

interface NodeLayout {
  nodeId: string
  position: { x: number; y: number; z: number }
  x: number
  y: number
  z: number
}

interface GraphNodesProps {
  graphData: GraphData
  nodePositions: NodeLayout[]
}

function NodeMesh({
  node,
  position,
  depthOffset,
  onClick,
  onPointerEnter,
  onPointerLeave,
  isSelected,
  isHovered,
}: any) {
  const meshRef = React.useRef<THREE.Mesh>(null)

  const size = useMemo(() => getNodeSize(node), [node])
  const glowIntensity = useMemo(() => getNodeGlowIntensity(node), [node])
  const opacity = useMemo(() => getNodeOpacity(node), [node])

  // Get color based on community or node type
  const color = useMemo(() => {
    if (node.community_id && COMMUNITY_COLORS[node.community_id]) {
      return COMMUNITY_COLORS[node.community_id]
    }
    // Fallback colors by type
    const typeColors: Record<string, string> = {
      memory: "#5B7FFF",
      entity: "#7FFF5B",
      community: "#FFD700",
      evidence: "#FF6B6B",
      agent_context: "#A855F7",
    }
    return typeColors[node.node_type] || "#CCCCCC"
  }, [node.community_id, node.node_type])

  const geometry = useMemo(() => getNodeGeometry(size), [size])

  const material = useMemo(() => {
    const baseMaterial = createNodeMaterial(color, color)
    baseMaterial.emissiveIntensity = isSelected ? 0.8 : glowIntensity
    baseMaterial.opacity = opacity
    baseMaterial.transparent = opacity < 1
    return baseMaterial
  }, [color, glowIntensity, isSelected, opacity])

  const glowMaterial = useMemo(() => {
    const mat = createGlowMaterial(color, glowIntensity)
    if (isSelected) {
      mat.opacity = 0.6
    }
    return mat
  }, [color, glowIntensity, isSelected])

  const adjustedZ = position.z + depthOffset

  return (
    <group position={[position.x, position.y, adjustedZ]}>
      {/* Main node sphere */}
      <mesh
        ref={meshRef}
        geometry={geometry}
        material={material}
        onClick={onClick}
        onPointerEnter={onPointerEnter}
        onPointerLeave={onPointerLeave}
      />

      {/* Glow layer (larger, behind main) */}
      {(isSelected || isHovered) && (
        <mesh geometry={geometry} material={glowMaterial} scale={1.2} position={[0, 0, -0.1]} />
      )}
    </group>
  )
}

export const GraphNodes: React.FC<GraphNodesProps> = ({ graphData, nodePositions }) => {
  const selectedNodeId = useGraphStore((s) => s.selectedNodeId)
  const hoveredNodeId = useGraphStore((s) => s.hoveredNodeId)
  const setSelectedNodeId = useGraphStore((s) => s.setSelectedNodeId)
  const setHoveredNodeId = useGraphStore((s) => s.setHoveredNodeId)
  const enabledNodeTypes = useGraphStore((s) => s.enabledNodeTypes)
  const showEvidence = useGraphStore((s) => s.showEvidence)

  // Compute depths once
  const depths = useMemo(() => computeNodeDepths(graphData), [graphData])
  const depthMap = useMemo(() => new Map(depths.map((d) => [d.nodeId, d])), [depths])

  // Filter nodes based on settings
  const visibleNodes = useMemo(() => {
    return graphData.nodes.filter((node) => {
      if (!enabledNodeTypes.has(node.node_type)) return false
      if (node.node_type === "evidence" && !showEvidence) return false
      return true
    })
  }, [graphData.nodes, enabledNodeTypes, showEvidence])

  const handleNodeClick = useCallback(
    (nodeId: string) => {
      setSelectedNodeId(nodeId === selectedNodeId ? null : nodeId)
    },
    [selectedNodeId, setSelectedNodeId]
  )

  const handleNodeHover = useCallback(
    (nodeId: string) => {
      setHoveredNodeId(nodeId)
    },
    [setHoveredNodeId]
  )

  const handleNodeHoverOut = useCallback(() => {
    setHoveredNodeId(null)
  }, [setHoveredNodeId])

  return (
    <>
      {visibleNodes.map((node) => {
        const nodePos = nodePositions.find((p) => p.nodeId === node.id)
        if (!nodePos) return null

        const depth = depthMap.get(node.id)
        const depthOffset = depth?.depthOffset ?? 0
        const isSelected = node.id === selectedNodeId
        const isHovered = node.id === hoveredNodeId

        return (
          <NodeMesh
            key={node.id}
            node={node}
            position={nodePos.position}
            depthOffset={depthOffset}
            isSelected={isSelected}
            isHovered={isHovered}
            onClick={() => handleNodeClick(node.id)}
            onPointerEnter={() => handleNodeHover(node.id)}
            onPointerLeave={handleNodeHoverOut}
          />
        )
      })}
    </>
  )
}
