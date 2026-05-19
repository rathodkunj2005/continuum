import { create } from "zustand"
import { devtools } from "zustand/middleware"
import type { GraphUIState, NodeType, EdgeType } from "../types"
import { NodeType as NT, EdgeType as ET } from "../types"

interface GraphStoreActions {
  setMode: (mode: "context" | "atlas") => void
  setSelectedNodeId: (id: string | null) => void
  setHoveredNodeId: (id: string | null) => void
  setZoomLevel: (zoom: number) => void
  setLoading: (loading: boolean) => void
  setError: (error: string | null) => void
  toggleNodeType: (type: NodeType) => void
  toggleEdgeType: (type: EdgeType) => void
  toggleCommunityFilter: (communityId: string) => void
  resetFilters: () => void
  toggleShowEvidence: () => void
  toggleShowLabels: () => void
}

export const useGraphStore = create<GraphUIState & GraphStoreActions>()(
  devtools(
    (set) => ({
      // Initial state
      mode: "atlas" as const,
      selectedNodeId: null,
      hoveredNodeId: null,
      expandedNodeIds: new Set(),
      selectedCommunityIds: new Set(),
      enabledNodeTypes: new Set([NT.Memory, NT.Entity, NT.Community]),
      enabledEdgeTypes: new Set([ET.SemanticSimilarity, ET.ExplicitReference, ET.SameProject, ET.TemporalAdjacency]),
      zoomLevel: 1,
      isLoading: false,
      error: null,
      showEvidence: false,
      showLabels: true,

      // Actions
      setMode: (mode: "context" | "atlas") => set({ mode }),
      setSelectedNodeId: (id: string | null) => set({ selectedNodeId: id }),
      setHoveredNodeId: (id: string | null) => set({ hoveredNodeId: id }),
      setZoomLevel: (zoom: number) => set({ zoomLevel: zoom }),
      setLoading: (loading: boolean) => set({ isLoading: loading }),
      setError: (error: string | null) => set({ error }),
      toggleNodeType: (type: NodeType) =>
        set((state: GraphUIState & GraphStoreActions) => {
          const next = new Set(state.enabledNodeTypes)
          next.has(type) ? next.delete(type) : next.add(type)
          return { enabledNodeTypes: next }
        }),
      toggleEdgeType: (type: EdgeType) =>
        set((state: GraphUIState & GraphStoreActions) => {
          const next = new Set(state.enabledEdgeTypes)
          next.has(type) ? next.delete(type) : next.add(type)
          return { enabledEdgeTypes: next }
        }),
      toggleCommunityFilter: (communityId: string) =>
        set((state: GraphUIState & GraphStoreActions) => {
          const next = new Set(state.selectedCommunityIds)
          next.has(communityId) ? next.delete(communityId) : next.add(communityId)
          return { selectedCommunityIds: next }
        }),
      resetFilters: () =>
        set({
          selectedCommunityIds: new Set(),
          expandedNodeIds: new Set(),
          selectedNodeId: null,
        }),
      toggleShowEvidence: () => set((state: GraphUIState & GraphStoreActions) => ({ showEvidence: !state.showEvidence })),
      toggleShowLabels: () => set((state: GraphUIState & GraphStoreActions) => ({ showLabels: !state.showLabels })),
    }),
    { name: "GraphStore" }
  )
)
