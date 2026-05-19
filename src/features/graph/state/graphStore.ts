import { create } from "zustand"
import { devtools } from "zustand/middleware"
import type { GraphUIState, NodeType, EdgeType } from "../types"

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
      enabledNodeTypes: new Set(["memory", "entity", "community"]),
      enabledEdgeTypes: new Set(["semantic_similarity", "explicit_reference", "same_project", "temporal_adjacency"]),
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
        set((state) => {
          const next = new Set(state.enabledNodeTypes)
          next.has(type) ? next.delete(type) : next.add(type)
          return { enabledNodeTypes: next }
        }),
      toggleEdgeType: (type: EdgeType) =>
        set((state) => {
          const next = new Set(state.enabledEdgeTypes)
          next.has(type) ? next.delete(type) : next.add(type)
          return { enabledEdgeTypes: next }
        }),
      toggleCommunityFilter: (communityId: string) =>
        set((state) => {
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
      toggleShowEvidence: () => set((state) => ({ showEvidence: !state.showEvidence })),
      toggleShowLabels: () => set((state) => ({ showLabels: !state.showLabels })),
    }),
    { name: "GraphStore" }
  )
)
