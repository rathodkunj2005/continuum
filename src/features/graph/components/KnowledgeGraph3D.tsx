import React, { useEffect, useState, useCallback } from "react"
import { graphDataAdapter } from "../data/adapter"
import { useGraphStore } from "../state/graphStore"
import type { GraphData } from "../types"
import { FocusType } from "../types"
import { GraphScene } from "./GraphScene"
import { GraphControls } from "./GraphControls"
import { GraphSidePanel } from "./GraphSidePanel"
import { GraphHoverCard } from "./GraphHoverCard"

interface KnowledgeGraph3DProps {
  onClose?: () => void
}

export const KnowledgeGraph3D: React.FC<KnowledgeGraph3DProps> = ({ onClose }) => {
  const [graphData, setGraphData] = useState<GraphData | null>(null)
  const [error, setError] = useState<string | null>(null)

  const mode = useGraphStore((s) => s.mode)
  const setMode = useGraphStore((s) => s.setMode)
  const setLoading = useGraphStore((s) => s.setLoading)
  const selectedNodeId = useGraphStore((s) => s.selectedNodeId)
  const hoveredNodeId = useGraphStore((s) => s.hoveredNodeId)
  const enabledNodeTypes = useGraphStore((s) => s.enabledNodeTypes)
  const enabledEdgeTypes = useGraphStore((s) => s.enabledEdgeTypes)

  // Track mount/unmount
  useEffect(() => {
    console.debug("[KnowledgeGraph3D] 🎬 Component mounted")
    return () => {
      console.debug("[KnowledgeGraph3D] 🎬 Component unmounted")
    }
  }, [])

  // Load initial graph data
  useEffect(() => {
    const loadGraphData = async () => {
      setLoading(true)
      setError(null)

      try {
        if (mode === "atlas") {
          const data = await graphDataAdapter.loadAtlasGraph()
          setGraphData(data)
        } else if (mode === "context") {
          // For context mode, use default focus if available
          const data = await graphDataAdapter.loadContextGraph({
            focus_type: FocusType.Atlas,
            label: "Full Memory Atlas",
          })
          setGraphData(data)
        }
      } catch (err) {
        const message = err instanceof Error ? err.message : "Failed to load graph"
        setError(message)
        console.error("Graph load error:", err)
      } finally {
        setLoading(false)
      }
    }

    loadGraphData()
  }, [mode, setLoading])

  const handleModeChange = useCallback(
    (newMode: "atlas" | "context") => {
      setMode(newMode)
      graphDataAdapter.clearCache()
    },
    [setMode]
  )

  if (error) {
    return (
      <div className="flex items-center justify-center w-full h-full bg-slate-900 rounded-lg">
        <div className="text-center">
          <p className="text-red-400 mb-4">Error loading graph</p>
          <p className="text-sm text-slate-400">{error}</p>
          {onClose && (
            <button
              onClick={onClose}
              className="mt-4 px-4 py-2 bg-slate-700 hover:bg-slate-600 rounded text-sm"
            >
              Close
            </button>
          )}
        </div>
      </div>
    )
  }

  if (!graphData) {
    return (
      <div className="flex items-center justify-center w-full h-full bg-slate-900 rounded-lg">
        <div className="text-center">
          <div className="w-8 h-8 border-2 border-slate-600 border-t-slate-300 rounded-full animate-spin mx-auto mb-4" />
          <p className="text-slate-400">Loading graph...</p>
        </div>
      </div>
    )
  }

  if (!graphData.nodes || graphData.nodes.length === 0) {
    return (
      <div className="flex items-center justify-center w-full h-full bg-slate-900 rounded-lg">
        <div className="text-center">
          <p className="text-slate-400 mb-4">No memories to visualize</p>
          <p className="text-xs text-slate-500">
            Start capturing or searching to build your memory graph
          </p>
          {onClose && (
            <button
              onClick={onClose}
              className="mt-4 px-4 py-2 bg-slate-700 hover:bg-slate-600 rounded text-sm"
            >
              Close
            </button>
          )}
        </div>
      </div>
    )
  }

  const selectedNode = graphData.nodes.find((n) => n.id === selectedNodeId)
  const hoveredNode = graphData.nodes.find((n) => n.id === hoveredNodeId)

  return (
    <div className="relative w-full h-full bg-slate-950 rounded-lg overflow-hidden flex flex-col">
      {/* Main graph canvas */}
      <div className="flex-1 relative">
        <GraphScene graphData={graphData} />

        {/* Hover card */}
        {hoveredNode && hoveredNodeId !== selectedNodeId && (
          <GraphHoverCard node={hoveredNode} />
        )}

        {/* Controls */}
        <GraphControls onModeChange={handleModeChange} graphData={graphData} />

        {/* Dev diagnostics */}
        {typeof window !== "undefined" && (
          <div className="absolute bottom-4 right-4 bg-slate-900 bg-opacity-95 border border-slate-700 p-3 rounded text-xs text-slate-300 font-mono max-w-xs">
            <div className="font-bold text-slate-100 mb-2">📊 Graph Diagnostics</div>
            <div className="space-y-1">
              <div>
                <span className="text-slate-500">Data Source:</span>{" "}
                <span className="text-cyan-400">
                  {graphData.nodes.length > 0 ? "backend_graph" : "empty"}
                </span>
              </div>
              <div>
                <span className="text-slate-500">Mode:</span> <span className="text-blue-400">{mode}</span>
              </div>
              <div className="border-t border-slate-700 pt-1 mt-1">
                <div>
                  <span className="text-slate-500">Nodes:</span> {graphData.nodes.length}
                </div>
                <div>
                  <span className="text-slate-500">Total Edges:</span> {graphData.edges.length}
                </div>
                <div>
                  <span className="text-slate-500">Communities:</span> {graphData.communities.length}
                </div>
              </div>
              <div className="border-t border-slate-700 pt-1 mt-1">
                <div>
                  <span className="text-slate-500">Node Filters:</span> {enabledNodeTypes.size}/3
                </div>
                <div>
                  <span className="text-slate-500">Edge Filters:</span> {enabledEdgeTypes.size}/7
                </div>
              </div>
              {selectedNodeId && (
                <div className="border-t border-slate-700 pt-1 mt-1">
                  <div>
                    <span className="text-slate-500">Selected:</span> {selectedNodeId.slice(0, 8)}
                  </div>
                </div>
              )}
              {hoveredNodeId && (
                <div>
                  <span className="text-slate-500">Hovered:</span> {hoveredNodeId.slice(0, 8)}
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Side panel */}
      {selectedNode && <GraphSidePanel node={selectedNode} graphData={graphData} />}

      {/* Close button */}
      {onClose && (
        <button
          onClick={onClose}
          className="absolute top-4 right-4 z-50 p-2 bg-slate-800 hover:bg-slate-700 rounded"
          title="Close graph"
        >
          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      )}
    </div>
  )
}
