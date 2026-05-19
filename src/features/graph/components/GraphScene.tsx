import React, { useRef, useEffect, useMemo } from "react"
import { Canvas, useFrame, useThree } from "@react-three/fiber"
import { OrbitControls, PerspectiveCamera } from "@react-three/drei"
import * as THREE from "three"
import type { GraphData } from "../types"
import { GraphNodes } from "./GraphNodes"
import { GraphEdges } from "./GraphEdges"
import { GraphLabels } from "./GraphLabels"
import { computeCommunityAnchors, computeLocalNodePositions } from "../layout/communityLayout"
import { useGraphStore } from "../state/graphStore"

interface GraphSceneProps {
  graphData: GraphData
}

function SceneContent({ graphData }: GraphSceneProps) {
  const { camera } = useThree()
  const controlsRef = useRef<any>(null)
  const hasMountedRef = useRef(false)
  const selectedNodeId = useGraphStore((s) => s.selectedNodeId)

  // Compute layout once — only changes when graphData actually changes
  const layout = useMemo(() => {
    console.debug("[GraphScene] Layout recompute")
    const communities = computeCommunityAnchors(graphData.communities)
    const nodePositions = computeLocalNodePositions(graphData.nodes, communities)
    return { communities, nodePositions }
  }, [graphData.nodes, graphData.communities])

  // Initialize camera position ONLY on first mount — do not reset on layout changes
  useEffect(() => {
    if (hasMountedRef.current) return

    console.debug("[GraphScene] Canvas mounted, initializing camera")
    hasMountedRef.current = true

    const bounds = new THREE.Box3()

    // Add all community anchors to bounds
    layout.communities.forEach((community) => {
      bounds.expandByPoint(
        new THREE.Vector3(community.anchor.x, community.anchor.y, community.anchor.z)
      )
    })

    // Add padding to frame all communities
    const size = bounds.getSize(new THREE.Vector3())
    const maxDim = Math.max(size.x, size.y, size.z)
    const fov = (camera as THREE.PerspectiveCamera).fov * (Math.PI / 180)
    let cameraZ = Math.abs(maxDim / 2 / Math.tan(fov / 2))
    cameraZ *= 1.5 // Add padding

    camera.position.set(0, 0, cameraZ)
    camera.lookAt(0, 0, 0)

    if (controlsRef.current) {
      controlsRef.current.target.set(0, 0, 0)
      controlsRef.current.update()
    }
  }, []) // Only run once on mount

  // Focus on selected node when explicitly selected (not on hover)
  // Use ref to avoid triggering on layout/nodePositions changes
  const selectedNodeRef = useRef<string | null>(null)
  useEffect(() => {
    if (selectedNodeId && selectedNodeId !== selectedNodeRef.current && controlsRef.current) {
      selectedNodeRef.current = selectedNodeId
      console.debug("[GraphScene] Focusing selected node:", selectedNodeId)
      const nodeLayout = layout.nodePositions.find((n) => n.nodeId === selectedNodeId)
      if (nodeLayout) {
        // Smooth camera transition to selected node
        controlsRef.current.target.lerp(
          new THREE.Vector3(nodeLayout.x, nodeLayout.y, nodeLayout.z),
          0.1
        )
        controlsRef.current.update()
      }
    } else if (!selectedNodeId) {
      selectedNodeRef.current = null
    }
  }, [selectedNodeId, layout.nodePositions])

  // Subtle animation loop
  useFrame(() => {
    if (controlsRef.current) {
      controlsRef.current.update()
    }
  })

  return (
    <>
      {/* Lighting */}
      <ambientLight intensity={0.5} color={0x8899aa} />
      <directionalLight position={[200, 200, 200]} intensity={0.8} color={0xffffff} />
      <directionalLight position={[-200, -100, -200]} intensity={0.3} color={0x6666ff} />

      {/* Fog for depth perception */}
      <fog attach="fog" args={[0x0a0e27, 500, 2000]} />

      {/* Scene background */}
      <color attach="background" args={[0x0a0e27]} />

      {/* Grid helper (subtle, optional) */}
      <gridHelper args={[400, 40, 0x444444, 0x222222]} position={[0, -200, 0]} />

      {/* Graph content */}
      <GraphNodes graphData={graphData} nodePositions={layout.nodePositions} />
      <GraphEdges graphData={graphData} nodePositions={layout.nodePositions} />
      <GraphLabels
        graphData={graphData}
        nodePositions={layout.nodePositions}
        communities={layout.communities}
      />

      {/* Controls */}
      <OrbitControls
        ref={controlsRef}
        makeDefault
        autoRotate={false}
        enableDamping
        dampingFactor={0.05}
        rotateSpeed={0.5}
        zoomSpeed={0.5}
        panSpeed={0.5}
        minDistance={50}
        maxDistance={2000}
        enablePan
        enableZoom
        enableRotate
      />
    </>
  )
}

export const GraphScene: React.FC<GraphSceneProps> = ({ graphData }) => {
  return (
    <Canvas
      camera={{ position: [0, 0, 400], fov: 75 }}
      className="w-full h-full"
      dpr={typeof window !== "undefined" ? window.devicePixelRatio : 1}
    >
      <PerspectiveCamera makeDefault position={[0, 0, 400]} fov={75} />
      <SceneContent graphData={graphData} />
    </Canvas>
  )
}
