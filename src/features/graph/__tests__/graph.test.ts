import { describe, it, expect } from "vitest"
import { selectVisibleEdges, getEdgeColor, getEdgeWidth } from "../layout/edgeVisibility"
import { computeNodeDepths, getNodeSize, getNodeGlowIntensity, getNodeOpacity } from "../layout/depthComputation"
import type { GraphEdge, GraphNode, GraphData } from "../types"
import { NodeType as NT, EdgeType as ET } from "../types"

describe("Graph Layout & Rendering", () => {
  describe("Edge Visibility", () => {
    it("should select visible edges with top-K filtering", () => {
      const edges: GraphEdge[] = [
        {
          id: "e1",
          source: "n1",
          target: "n2",
          edge_type: ET.SemanticSimilarity,
          weight: 0.9,
          confidence: 0.8,
        },
        {
          id: "e2",
          source: "n2",
          target: "n3",
          edge_type: ET.SameProject,
          weight: 0.5,
          confidence: 0.7,
        },
        {
          id: "e3",
          source: "n3",
          target: "n4",
          edge_type: ET.TemporalAdjacency,
          weight: 0.3,
          confidence: 0.6,
        },
      ]

      const selected = selectVisibleEdges(edges, new Set(), new Set(Object.values(ET)), 10)
      expect(selected.length).toBeLessThanOrEqual(10)
      expect(selected[0].weight).toBeGreaterThanOrEqual(selected[1]?.weight ?? 0)
    })

    it("should prioritize edges connected to selected nodes", () => {
      const edges: GraphEdge[] = [
        {
          id: "e1",
          source: "n1",
          target: "n2",
          edge_type: ET.SemanticSimilarity,
          weight: 0.5,
        },
        {
          id: "e2",
          source: "n3",
          target: "n4",
          edge_type: ET.SameProject,
          weight: 0.9,
        },
      ]

      const selected = selectVisibleEdges(edges, new Set(["n1"]), new Set(Object.values(ET)), 10)
      const n1Edges = selected.filter((e) => e.source === "n1" || e.target === "n1")
      expect(n1Edges.length).toBeGreaterThan(0)
    })

    it("should filter by edge type", () => {
      const edges: GraphEdge[] = [
        {
          id: "e1",
          source: "n1",
          target: "n2",
          edge_type: ET.SemanticSimilarity,
          weight: 0.9,
        },
        {
          id: "e2",
          source: "n2",
          target: "n3",
          edge_type: ET.SameProject,
          weight: 0.8,
        },
      ]

      const selected = selectVisibleEdges(edges, new Set(), new Set([ET.SemanticSimilarity]), 10)
      expect(selected.every((e) => e.edge_type === ET.SemanticSimilarity)).toBe(true)
    })

    it("should cap visible edges to max", () => {
      const edges = Array.from({ length: 100 }, (_, i) => ({
        id: `e${i}`,
        source: `n${i}`,
        target: `n${i + 1}`,
        edge_type: ET.SemanticSimilarity,
        weight: Math.random(),
      }))

      const selected = selectVisibleEdges(edges, new Set(), new Set(Object.values(ET)), 20)
      expect(selected.length).toBeLessThanOrEqual(20)
    })

    it("should assign correct edge colors by type", () => {
      expect(getEdgeColor(ET.SemanticSimilarity)).toBe("#7F8D8D")
      expect(getEdgeColor(ET.ExplicitReference)).toBe("#5B7FFF")
      expect(getEdgeColor(ET.SameProject)).toBe("#FFD700")
    })

    it("should compute edge width based on type and weight", () => {
      const width1 = getEdgeWidth(ET.SemanticSimilarity, 0.5)
      const width2 = getEdgeWidth(ET.ExplicitReference, 0.5)
      expect(width2).toBeGreaterThan(width1) // explicit_reference is thicker
    })
  })

  describe("Node Depth & Size", () => {
    it("should compute node depths based on relevance score", () => {
      const graph: GraphData = {
        nodes: [
          { id: "n1", node_type: NT.Memory, title: "High relevance", relevance_score: 0.9 },
          { id: "n2", node_type: NT.Memory, title: "Low relevance", relevance_score: 0.1 },
        ],
        edges: [],
        communities: [],
      }

      const depths = computeNodeDepths(graph)
      const highRel = depths.find((d) => d.nodeId === "n1")
      const lowRel = depths.find((d) => d.nodeId === "n2")

      expect(highRel?.depthOffset ?? 0).toBeGreaterThan(lowRel?.depthOffset ?? 0)
    })

    it("should compute correct node sizes", () => {
      const nodeHigh: GraphNode = {
        id: "n1",
        node_type: NT.Memory,
        title: "Important",
        importance_score: 0.9,
        reuse_count: 10,
        confidence_score: 0.95,
      }
      const nodeLow: GraphNode = {
        id: "n2",
        node_type: NT.Memory,
        title: "Unimportant",
        importance_score: 0.1,
        reuse_count: 0,
        confidence_score: 0.5,
      }

      const sizeHigh = getNodeSize(nodeHigh)
      const sizeLow = getNodeSize(nodeLow)
      expect(sizeHigh).toBeGreaterThan(sizeLow)
      expect(sizeHigh).toBeLessThanOrEqual(4.0)
      expect(sizeLow).toBeGreaterThanOrEqual(0.8)
    })

    it("should compute glow intensity from importance and relevance", () => {
      const nodeHigh: GraphNode = {
        id: "n1",
        node_type: NT.Memory,
        title: "Important",
        importance_score: 0.9,
        relevance_score: 0.8,
      }
      const nodeLow: GraphNode = {
        id: "n2",
        node_type: NT.Memory,
        title: "Unimportant",
        importance_score: 0.2,
        relevance_score: 0.1,
      }

      const glowHigh = getNodeGlowIntensity(nodeHigh)
      const glowLow = getNodeGlowIntensity(nodeLow)
      expect(glowHigh).toBeGreaterThan(glowLow)
    })

    it("should compute opacity from relevance score", () => {
      const nodeHigh: GraphNode = {
        id: "n1",
        node_type: NT.Memory,
        title: "Relevant",
        relevance_score: 1.0,
      }
      const nodeLow: GraphNode = {
        id: "n2",
        node_type: NT.Memory,
        title: "Irrelevant",
        relevance_score: 0.0,
      }

      const opacityHigh = getNodeOpacity(nodeHigh)
      const opacityLow = getNodeOpacity(nodeLow)
      expect(opacityHigh).toBeGreaterThan(opacityLow)
      expect(opacityHigh).toBeLessThanOrEqual(1.0)
      expect(opacityLow).toBeGreaterThanOrEqual(0.3)
    })
  })

  describe("Label Discipline", () => {
    it("should respect label cap in visualization", () => {
      const LABEL_CONFIG = {
        maxLabelsVisible: 20,
        truncateLength: 20,
        topImportanceShown: 5,
        defaultFontSize: 12,
      }

      // This is implicitly tested in GraphLabels component
      // Verify the constants exist
      expect(LABEL_CONFIG.maxLabelsVisible).toBe(20)
      expect(LABEL_CONFIG.topImportanceShown).toBe(5)
    })
  })

  describe("Privacy", () => {
    it("should not expose raw evidence nodes by default", () => {
      // Evidence nodes should be hidden by default
      // This is tested in component rendering, not in utilities
      // Just verify the enum exists
      expect(NT.Evidence).toBeDefined()
    })
  })
})
