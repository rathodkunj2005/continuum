import React, { useEffect, useState, useCallback, useMemo, useRef } from "react"
import type { InsightGraphSubgraph } from "@/shared/ipc/tauri"
import { graphDataAdapter } from "../data/adapter"
import { normalizeInsightGraph } from "../data/normalizeInsightGraph"
import { computeCommunityAnchors, computeLocalNodePositions } from "../layout/communityLayout"
import { useGraphStore } from "../state/graphStore"
import type { GraphData } from "../types"
import { FocusType } from "../types"
import { GraphScene } from "./GraphScene"
import { GraphControls } from "./GraphControls"
import { GraphSidePanel } from "./GraphSidePanel"
import { GraphHoverCard } from "./GraphHoverCard"
import { GraphLabels } from "./GraphLabels"

interface KnowledgeGraph3DProps {
  onClose?: () => void
  /** Optional bridged data from the 2D graph. When provided with nodes, used in place of the
   *  (currently stubbed) backend atlas command. */
  subgraph?: InsightGraphSubgraph | null
  louvain?: Record<string, number> | null
}

type DataSource = "bridged_2d_subgraph" | "backend_atlas" | "empty" | "error"

export const KnowledgeGraph3D: React.FC<KnowledgeGraph3DProps> = ({
  onClose,
  subgraph,
  louvain,
}) => {
  const [graphData, setGraphData] = useState<GraphData | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [dataSource, setDataSource] = useState<DataSource>("empty")

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

  // Stable reference to the subgraph identity so we don't re-normalize on every render.
  // We only care when the size of the underlying data actually changes.
  const subgraphKey = useMemo(() => {
    if (!subgraph) return null
    return `${subgraph.nodes.length}:${subgraph.edges.length}:${subgraph.cluster_0_name ?? ""}`
  }, [subgraph])

  // Keep refs in sync with unstable object props so we can read them inside effects
  // without triggering re-runs when the parent re-renders.
  const subgraphRef = useRef<InsightGraphSubgraph | null>(null)
  const louvainRef = useRef<Record<string, number> | null>(null)

  useEffect(() => {
    subgraphRef.current = subgraph ?? null
  }, [subgraph])

  useEffect(() => {
    louvainRef.current = louvain ?? null
  }, [louvain])

  // Load graph data — priority: bridged subgraph > backend atlas command > error
  useEffect(() => {
    let cancelled = false

    const load = async () => {
      setLoading(true)
      setError(null)

      // Priority 1: bridged 2D subgraph
      if (subgraphRef.current && subgraphRef.current.nodes.length > 0) {
        const data = normalizeInsightGraph(subgraphRef.current, louvainRef.current)
        if (cancelled) return
        console.debug(
          `[KnowledgeGraph3D] data source: bridged 2D subgraph ${data.nodes.length} nodes`
        )
        setDataSource("bridged_2d_subgraph")
        setGraphData(data)
        setLoading(false)
        return
      }

      // Priority 2: backend atlas command (currently stubbed; will work when implemented)
      try {
        const data =
          mode === "context"
            ? await graphDataAdapter.loadContextGraph({
                focus_type: FocusType.Atlas,
                label: "Full Memory Atlas",
              })
            : await graphDataAdapter.loadAtlasGraph()
        if (cancelled) return
        console.debug(
          `[KnowledgeGraph3D] data source: backend get_memory_graph_atlas ${data.nodes.length} nodes`
        )
        setDataSource(data.nodes.length > 0 ? "backend_atlas" : "empty")
        setGraphData(data)
      } catch (err) {
        if (cancelled) return
        const message = err instanceof Error ? err.message : "Failed to load graph"
        setError(message)
        setDataSource("error")
        console.error("[KnowledgeGraph3D] graph load error:", err)
      } finally {
        if (!cancelled) setLoading(false)
      }
    }

    void load()
    return () => {
      cancelled = true
    }
  }, [subgraphKey, mode, setLoading])

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
    const enabledNodeTypeCount = enabledNodeTypes.size
    const enabledEdgeTypeCount = enabledEdgeTypes.size
    const looksFilteredOut = enabledNodeTypeCount < 3 || enabledEdgeTypeCount < 7
    return (
      <div className="flex items-center justify-center w-full h-full bg-slate-900 rounded-lg">
        <div className="text-center">
          <p className="text-slate-400 mb-4">
            {looksFilteredOut
              ? "No visible graph nodes match the current filters."
              : "No graph data available."}
          </p>
          <p className="text-xs text-slate-500">
            {looksFilteredOut
              ? "Try resetting filters from the controls panel."
              : "Capture some memories to build your knowledge graph."}
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

  // Compute layout for labels (same as in GraphScene, but needed here for GraphLabels outside Canvas)
  const { communities, nodePositions } = useMemo(() => {
    const communities = computeCommunityAnchors(graphData.communities)
    const nodePositions = computeLocalNodePositions(graphData.nodes, communities)
    return { communities, nodePositions }
  }, [graphData.nodes, graphData.communities])

  return (
    <div className="relative w-full h-full bg-slate-950 rounded-lg overflow-hidden flex flex-col">
      {/* Main graph canvas */}
      <div className="flex-1 relative">
        <GraphScene graphData={graphData} />

        {/* Labels layer — positioned absolutely over canvas (DOM, NOT inside Canvas) */}
        <div className="absolute inset-0 pointer-events-none">
          <GraphLabels
            graphData={graphData}
            nodePositions={nodePositions}
            communities={communities}
          />
        </div>

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
                <span className="text-cyan-400">{dataSource}</span>
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
