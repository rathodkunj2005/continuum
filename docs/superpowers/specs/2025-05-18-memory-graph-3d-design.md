# Continuum Memory Graph 3D: Design Specification

**Date:** 2025-05-18  
**Feature:** MemoryGraph3D / KnowledgeGraph3D  
**Status:** Design Draft — Pending Implementation Plan  
**Primary Goal:** Build a beautiful, spatially meaningful 3D knowledge graph that helps users navigate their second brain.

---

## 1. Design Philosophy

Continuum's graph should be a **spatially meaningful second brain map**, not a chaotic particle visualization. The graph answers four fundamental questions:

- **What does Continuum know?** (Atlas Mode)
- **Where is this memory located semantically?** (Context Mode + visual positioning)
- **Why is this memory connected?** (Explainability layer)
- **What evidence supports it?** (Progressive disclosure)
- **What can an agent do with this context?** (Side panel actions)

### Core Principles

1. **Spatial Memory** — Users learn communities over time (Design, Code, Meetings, etc.).
2. **Stability** — Global layout is deterministic and stable; local motion is organic.
3. **Progressive Disclosure** — Complexity is hidden; details appear on demand.
4. **Performance First** — Graph feels responsive before fancy.
5. **Explainability** — Every connection has a reason.
6. **Agent Integration** — Graph is a thinking tool, not just a visualization.

---

## 2. Architecture Overview

### 2.1 Five-Level Hierarchy

```
LEVEL 1: Active Focus
├─ Current query, project, or selected memory
├─ Center point in Context Mode
└─ Optional in Atlas Mode

LEVEL 2: Anchored Semantic Communities
├─ Stable 3D orbital regions (not rings—constellations)
├─ Examples: Work/Code, Research, Design, Meetings, Errors, People, Files, Decisions, Todos, Concepts, Past Searches, Agent Context
├─ Pre-computed stable anchor positions
└─ Do not reshuffle between renders

LEVEL 3: Memory & Entity Nodes
├─ Memory cards, entities (people, apps, files, URLs, projects)
├─ Lightweight local force simulation within each community
├─ Attracted to community anchor, repel from neighbors
└─ Visible by default in their community region

LEVEL 4: Evidence & Provenance
├─ Source memories, confidence scores, relationship reasons
├─ Hidden by default, revealed on hover/click/filter
└─ Prevents graph clutter

LEVEL 5: Agent Actions & Controls
├─ Build context pack, send to agent, search around, focus, pin/boost, hide
├─ Lives in side panel, not cluttering the graph
└─ Optional, for advanced interaction
```

### 2.2 Two Graph Modes

#### **Context Mode**
- **Use when:** Searching, selecting a project, selecting a memory, building agent context
- **Center:** Active focus (query, project, or memory) sits near center/front
- **Behavior:**
  - Relevant communities move toward camera and brighten
  - Irrelevant communities fade back but stay visible
  - Z-depth primarily represents relevance to the active focus
  - Clicking a node opens side panel with details
  - Hovering reveals local neighborhood and strongest reasons for connection

#### **Atlas Mode**
- **Use when:** Browsing entire memory space, understanding long-term patterns, discovering forgotten work
- **Center:** No single focus dominates
- **Behavior:**
  - Communities remain in stable anchored positions
  - All nodes visible (filtered by importance/LOD)
  - Zoomed out view shows community names and aggregate node counts
  - Zooming in progressively reveals individual memory/entity nodes
  - Good for serendipitous discovery

---

## 3. Layout System

### 3.1 Global Layout: Anchored Orbital Regions

Each semantic community gets a **stable, pre-determined 3D position** arranged as orbital regions around the center.

**Why not random:**
- Users develop spatial memory for "Design is always at 45°, Code is always at 135°"
- Stable layout means the graph is learnable and navigable
- Do not shuffle community positions between renders

**Implementation:**
- Define canonical community order (or derive from data)
- Assign deterministic 3D anchor positions using spherical coordinates
- Examples: (radius=150, latitude=45°, longitude=0°) for Code, (radius=150, latitude=45°, longitude=120°) for Design, etc.
- If backend provides community data, use it; otherwise derive from available fields (project, topic, activityType, appName)

**Fallback Community Derivation:**
1. Explicit backend community ID (if available)
2. Project field → "Work: {ProjectName}"
3. Topic field → Topic value
4. activityType field → Activity name
5. appName field → "App: {AppName}"
6. Inferred bucket (e.g., "Meetings" if window title hints at conferencing)
7. Default → "Uncategorized"

### 3.2 Local Layout: Force Simulation Within Communities

Inside each community, nodes use lightweight force simulation:

- **Attraction:** Each node is weakly attracted to its community anchor
- **Repulsion:** Nodes repel nearby nodes within the same community
- **Edges:** Mild attraction between connected nodes
- **Stability:** Precompute initial positions, then interpolate to final layout (do not run physics every frame)

**Implementation:**
- Seed initial node positions deterministically (e.g., using hash of node ID)
- Run 20–50 iterations of local force simulation per community
- Memoize layout results; recompute only on data changes or explicit refresh
- Use smooth interpolation (TWEEN or Framer Motion) to animate nodes into final positions

### 3.3 Z-Depth Semantics

**Do not overload z-position.** Keep it clean and predictable.

**Primary driver:**
- **relevanceScore** → Directly controls z-position (high relevance moves forward)

**Secondary modifiers:**
- **importanceScore** → Affects node size, glow, label priority, and subtle brightness (not z-jumps)
- **recency** → Affects subtle freshness halo, animation tempo, or gentle drift (old memories may drift slightly back if low-reuse)

**Z-Depth Computation:**
```
baseZ = communityAnchor.z
contextOffset = normalize(relevanceScore) * forwardDepth  // +0 to +100
staleOffset = (lowRelevance AND lowReuse AND old) ? -smallBackwardDepth : 0  // -20 max
finalZ = baseZ + contextOffset + staleOffset
```

**Rules:**
- Old memories drift backward **only if** they are also low relevance and low reuse
- Frequently reused old memories remain prominent (staleOffset = 0)
- In Atlas Mode, all nodes at similar z-depth unless explicitly highlighted

---

## 4. Data Model

### 4.1 GraphNode

```typescript
interface GraphNode {
  id: string
  type: "memory" | "entity" | "community" | "evidence" | "agent_context"
  title: string
  summary?: string
  communityId?: string
  
  // Timestamps
  timestampStart?: string  // ISO 8601
  timestampEnd?: string
  
  // Source/Context
  appName?: string
  windowTitle?: string
  url?: string
  project?: string
  topic?: string
  activityType?: string
  
  // Scoring
  importanceScore?: number  // 0–1, affects size/glow
  relevanceScore?: number   // 0–1, affects z-depth (context mode)
  confidenceScore?: number  // 0–1, affects label/edge emphasis
  reuseCount?: number       // How many times used in agent context
  
  // Relationships
  sourceIds?: string[]      // Provenance: which memories this derives from
  
  // Custom
  metadata?: Record<string, unknown>
}
```

### 4.2 GraphEdge

```typescript
interface GraphEdge {
  id: string
  source: string  // node ID
  target: string  // node ID
  type: 
    | "semantic_similarity"
    | "explicit_reference"
    | "temporal_adjacency"
    | "same_project"
    | "same_session"
    | "agent_inferred"
    | "provenance"
  
  weight: number              // 0–1, strength of connection
  confidence?: number         // 0–1, how sure we are
  reason?: string            // One-line explanation
  
  metadata?: Record<string, unknown>
}
```

### 4.3 GraphCommunity

```typescript
interface GraphCommunity {
  id: string
  label: string             // "Code", "Design", "Meetings", etc.
  description?: string
  colorToken?: string       // e.g., "token-gold", "token-blue"
  anchor: { x: number; y: number; z: number }
  nodeCount?: number        // Cached count
  importanceScore?: number  // Aggregate importance
}
```

### 4.4 GraphData

```typescript
interface GraphData {
  nodes: GraphNode[]
  edges: GraphEdge[]
  communities: GraphCommunity[]
  activeFocus?: {
    type: "query" | "project" | "memory" | "agent_task" | "atlas"
    id?: string           // Node ID if focused on a memory
    label: string         // Display label
    query?: string        // Search query if applicable
  }
}
```

---

## 5. Visual Style & Rendering

### 5.1 Design Tokens

Continuum aesthetic is **dark, cinematic, premium**. Avoid generic "AI neon purple/blue."

**Base:**
- Background: Deep navy/black with subtle depth fog
- Primary text: Off-white
- Accent: Muted gold, green, or project-specific colors

**Per-community color:**
- Use existing design tokens if available
- Fallback: Assign consistent colors based on community ID hash
- Light enough for dark backgrounds, not oversaturated

### 5.2 Node Rendering

**Size:**
- Primary driver: `importanceScore` (or reuse count, connection count)
- Secondary: Selected/hovered nodes slightly larger
- Clamp to readable range: 0.8–4.0 units

**Color:**
- Base: Community color
- Brightness: Scaled by `importanceScore`
- Opacity: Scaled by `relevanceScore` (in Context Mode)

**Appearance:**
- Sphere or slightly faceted sphere (not flat discs)
- Subtle glow/bloom on important nodes
- Selected node: Pulsing glow or distinct outline
- Evidence nodes: Small, dim, hidden unless expanded

### 5.3 Edge Rendering

**Sparse by default.** Do not render all edges at once.

**Default visible edges:**
- Top K strongest edges per selected/hovered node (K=3–5)
- Top aggregate inter-community edges
- High-confidence explicit references
- Current query's strongest semantic paths

**Progressive reveal:**
- On hover: Show selected node's immediate neighborhood (1-hop edges)
- On click/expand: Show 1-hop + strongest 2-hop summaries
- On filter: Show edges matching selected edge type
- On zoom-in: Gradually reveal more local edges
- On zoom-out: Collapse edges into community-level connections

**Visual styles:**
- `semantic_similarity`: Soft, thin line (opacity 0.4)
- `explicit_reference` / `provenance`: Sharper, brighter line (opacity 0.8)
- `temporal_adjacency`: Faint, trail-like line (opacity 0.3)
- `same_project` / `same_session`: Medium line (opacity 0.6)
- `agent_inferred`: Dotted or distinct opacity treatment (opacity 0.5)

### 5.4 Label Rendering

**Strict discipline.** Labels clutter the graph. Use progressive disclosure.

**Default visible labels:**
- Community anchor labels (always, if not cluttered)
- Selected node label
- Hovered node label
- Top 3–5 most important visible nodes

**Zoomed out (far camera):**
- Community labels only
- Optional aggregate node counts per community

**Zoomed in (close camera):**
- Memory/entity labels reveal gradually as user zooms
- Truncate long labels (max 20 chars, suffix with "…")
- Never let labels flood the view

**Hover card** (not on graph):
- Node title
- App name / source
- Timestamp (relative, e.g., "2 hours ago")
- Short summary (1 line if available)
- Confidence/relevance if relevant
- One-line "why connected" reason (e.g., "Same project: Continuum")

**Full detail belongs in side panel, not directly on graph.**

---

## 6. Interaction Model

### 6.1 Camera & Navigation

**Controls (Orbit):**
- Left mouse drag: Rotate around center
- Right mouse drag or two-finger pan: Pan camera
- Scroll wheel: Zoom in/out
- Double-click node: Focus node (smooth camera transition)
- Double-click community: Focus community
- "Reset Camera" button: Return to initial view

**Constraints:**
- Soft orbit (not arbitrary free rotation)
- Zoom limits: Prevent zooming into the graph or too far away
- Inertial panning: Smooth deceleration on release
- "Return to Center" button: Recenter on active focus
- Easy "Focus Selected" behavior: Bring selected node into readable view (not exact center)
- Keep transitions smooth and under 0.5 seconds

### 6.2 Node Interaction

**Hover:**
- Highlight node and its immediate neighborhood (1-hop edges)
- Show hover card with title, app, timestamp, reason
- Fade other nodes slightly

**Click:**
- Select node
- Open side panel with full details
- Move node closer to camera (if not already visible)

**Double-click:**
- Focus camera on node (smooth transition)
- Optionally open side panel

### 6.3 Community Interaction

**Hover on community anchor:**
- Highlight all nodes in that community
- Show community name and node count

**Click on community anchor:**
- Focus camera on that community
- Option to filter graph to show only that community

### 6.4 Graph Controls

**Mode toggle:**
- Switch between Context Mode and Atlas Mode

**Filters:**
- Filter by community (checkboxes or dropdown)
- Filter by node type (memory, entity, evidence, etc.)
- Filter by edge type (semantic, explicit, temporal, etc.)
- Filter by importance threshold (slider)

**Search integration:**
- If a search query exists, highlight matching nodes
- Use query to compute relevance scores
- Offer "search around this node" action

---

## 7. Side Panel

When a node is selected, show:

**Header:**
- Node title
- Close button

**Details:**
- Summary (short)
- App name / window title / URL / project / topic (metadata)
- Timestamp (creation + modification if available)
- Importance score (visual bar)
- Relevance score (if in Context Mode)
- Confidence score (if available)

**Relationships:**
- Top connected nodes (5–10 strongest)
- For each: node title, edge type, reason

**Evidence / Provenance** (expand/collapse):
- Source memories (if this is a derived/summarized node)
- Confidence breakdown
- "Why this edge?" explanations

**Actions:**
- Build context pack (if agent actions available)
- Send to agent / Ask about this
- Search around this node (requery with this node as focus)
- Focus graph here (camera zoom)
- Pin/boost memory (if existing app supports)
- Hide/dismiss (if existing app supports)
- Copy ID or link

**Do not implement destructive mutations** unless existing safe commands exist. Prefer read-only actions.

---

## 8. Explainability Layer

Every highlighted node, edge, or cluster should be explainable.

### 8.1 Explanation Utility

Create a simple function that generates human-readable explanations:

```typescript
function explainConnection(
  sourceNode: GraphNode,
  targetNode: GraphNode,
  edge: GraphEdge
): string {
  // Examples:
  // "Connected because both memories belong to the Continuum project."
  // "Pulled forward due to high semantic similarity to your query."
  // "Frequently reused in prior agent context packs."
  // "Occurred in the same capture session."
  // "Related by explicit reference in source code."
  // etc.
}
```

### 8.2 Signal Examples

- **Semantic match:** "Pulled forward because it has high semantic similarity (0.89) to your query."
- **Project/topic:** "Connected because both memories belong to the Continuum project."
- **Temporal:** "Related by temporal proximity—captured in the same session."
- **Reuse:** "Frequently reused in prior agent context packs."
- **Reference:** "Connected through an explicit reference in the source code."
- **Graph:** "Connected by the knowledge graph through the MOTOR_CORTEX entity."

Show explanations in hover card and side panel.

---

## 9. Performance & Level-of-Detail

Continuum graphs may grow large. Implement smart LOD.

### 9.1 Rendering LOD

- **Zoomed far out (atlas view):**
  - Aggregate communities into single proxy nodes
  - Hide most individual nodes except highest importance
  - Show only top-K inter-community edges
  - Render simplified community outlines

- **Zoomed mid (typical view):**
  - Show all communities and important nodes
  - Show top-K edges per node (K=3–5)
  - Render labels for top communities + selected node
  - Full interaction available

- **Zoomed in (detail view):**
  - Show all local nodes in focused community
  - Reveal more edges within local neighborhood
  - Render all node labels in view
  - Optional evidence node reveal

### 9.2 Data LOD

- **Node count capped:** If > 500 nodes, aggregate by community until < 500 visible
- **Edge count capped:** If > 1000 edges, use top-K weighting per node
- **Evidence nodes:** Hidden by default, loaded on demand when user expands
- **Labels:** Never render all labels at once; prioritize by importance

### 9.3 Performance Targets

- **Smooth:** 60 FPS for typical graph (200–400 nodes)
- **Graceful:** Maintains 30+ FPS for large graphs (500+ nodes) with LOD
- **Responsive:** Search/filter updates in < 200ms
- **Loading:** Async data fetch with loading spinner
- **No freeze:** Graph interaction never blocks main thread

### 9.4 Implementation Techniques

- Use **instanced rendering** for similar nodes (sphere geometry)
- Use **WebGL directly** for labels (not DOM)
- **Memoize layout results** (recompute only on data change)
- **Debounce search/filter** (delay layout recompute for 200ms after input)
- **Avoid continuous simulation** after layout stabilizes (only on demand)
- **Use requestAnimationFrame** responsibly; batch updates

---

## 10. Empty & Error States

### 10.1 Empty Graph

If no memories exist:
- Show calm, welcoming empty graph state
- "No memories yet. Start capturing context and they'll appear here."
- No fake sample data (unless explicitly in dev mode)

### 10.2 No Search Results

If search query returns no matches:
- Keep atlas faintly visible
- Show "No strong memory matches for that query"
- Suggest: "Try a broader search or browse the full atlas"
- Offer "clear search" button

### 10.3 Loading State

- Show loading spinner or progress indicator
- Keep previous graph visible but dimmed (optional)
- Once data loads, smoothly transition

### 10.4 Error State

- Show user-friendly error message
- Offer "retry" button
- Include error details in development mode
- Do not crash the app

---

## 11. Backend Graph Projection Layer

### 11.1 Architecture Principle

**MemoryRecord / MemoryEvent remains the source of truth.** The graph system should expose **read-optimized projections** that the frontend consumes directly.

**The frontend should NOT derive graph structure from memory cards or search results as its primary path.** Frontend fallback derivation is acceptable only as a temporary compatibility bridge.

### 11.2 Backend Graph Commands (Tauri/Rust)

Implement a graph projection module in the Rust backend that produces `GraphData` directly:

```rust
// Tauri commands to implement
pub async fn get_memory_graph_atlas(
    params: AtlasGraphParams,
) -> Result<GraphData, Error>

pub async fn get_memory_graph_context(
    focus: ActiveFocus,
    params: ContextGraphParams,
) -> Result<GraphData, Error>

pub async fn get_graph_node_neighborhood(
    node_id: String,
    depth: u32,
    edge_types: Option<Vec<EdgeType>>,
) -> Result<GraphData, Error>

pub async fn get_graph_communities() -> Result<Vec<GraphCommunity>, Error>

pub async fn build_agent_context_from_graph_selection(
    node_ids: Vec<String>,
) -> Result<AgentContextPack, Error>  // If supported
```

### 11.3 Backend Projection Responsibilities

The backend graph module should:

1. **Derive communities** from available structured data:
   - Explicit project field → Project community
   - Topic field → Topic community
   - activityType field → Activity community
   - appName field → App community
   - Inferred type buckets → Inferred community
   - Fallback → "Uncategorized"

2. **Derive conservative edges** from relationships:
   - Same project → `same_project` edge
   - Same topic → `same_project` edge (topic as pseudo-project)
   - Same session/capture event → `same_session` edge
   - Temporal closeness (< 5 min) → `temporal_adjacency` edge
   - Explicit entity references → `explicit_reference` edge
   - Embeddings-based semantic similarity (if available) → `semantic_similarity` edge
   - Keep derivation conservative; avoid fake-looking overconnection

3. **Compute scores** from memory data:
   - `importanceScore`: Based on reuse count, frequency, or explicit importance field
   - `relevanceScore`: Computed per-query (context mode only), based on semantic similarity
   - `confidenceScore`: Based on entity detection confidence, data freshness, or explicit confidence field

4. **Provide layout hints** (optional):
   - Stable community anchors (computed once, cached)
   - Stable node positions within communities (optional precompute)
   - Allow frontend to override for smooth transitions

5. **Filter for privacy**:
   - Respect existing privacy/incognito/blocklist behavior
   - Do not expose raw screenshot content or sensitive fields
   - Evidence layer is optional and can be filtered per-user

6. **Enforce edge caps**:
   - Do not return all edges; use top-K weighting
   - Example: Max 10 edges per node, max 500 edges total
   - Progressive reveal on frontend request

7. **Support both modes**:
   - `get_memory_graph_atlas()` → Full graph without a single focus
   - `get_memory_graph_context(focus)` → Graph with relevance scores relative to focus

### 11.4 Shared Contract (Rust + TypeScript)

Define `GraphData` and related types in a shared contract file (or generated from Rust):

**Rust side:**
```rust
// src-tauri/src/graph/types.rs (or similar)
pub struct GraphNode { /* ... */ }
pub struct GraphEdge { /* ... */ }
pub struct GraphCommunity { /* ... */ }
pub struct GraphData { /* ... */ }
pub enum EdgeType { /* ... */ }
```

**TypeScript side:**
```typescript
// src/features/graph/types.ts (generated or hand-maintained)
export interface GraphNode { /* ... */ }
export interface GraphEdge { /* ... */ }
export interface GraphCommunity { /* ... */ }
export interface GraphData { /* ... */ }
export enum EdgeType { /* ... */ }
```

Use tools like `ts-rs` or hand-maintenance to keep these in sync.

### 11.5 Frontend Graph Data Adapter

The frontend `GraphDataAdapter` should call backend graph commands **first**:

```typescript
interface GraphDataAdapter {
  loadAtlasGraph(): Promise<GraphData> {
    // Call get_memory_graph_atlas() first
  }
  
  loadContextGraph(focus: ActiveFocus): Promise<GraphData> {
    // Call get_memory_graph_context(focus) first
  }
  
  // Fallback-only: derive from memory cards if backend projection not yet available
  // Use sparingly; not the primary path
  mapMemoryCardsToGraphData(cards: MemoryCard[]): GraphData
  mapSearchResultsToGraphData(results: SearchResult[]): GraphData
}
```

**Usage rule:**
1. Try to call backend graph command
2. If backend command not available → Fall back to memory card derivation
3. Cache result with TTL to avoid repeated computation

### 11.6 Privacy & Data Safety

- Do NOT expose raw OCR, screenshots, or sensitive metadata in `GraphNode`
- Do NOT include full window titles or URLs unless explicitly approved
- Evidence nodes must be filtered: hide by default, reveal only on user action
- Respect existing Continuum privacy/incognito behavior
- Do not create new data retention or backup of graph data

---

## 12. Component Structure

### 12.1 Directory Layout

```
src/features/graph/
├── components/
│   ├── KnowledgeGraph3D.tsx           # Main wrapper
│   ├── GraphScene.tsx                 # ThreeJS scene setup
│   ├── GraphNodes.tsx                 # Node rendering
│   ├── GraphEdges.tsx                 # Edge rendering
│   ├── GraphLabels.tsx                # Label rendering (canvas or Drei)
│   ├── GraphControls.tsx              # Mode, filter, zoom buttons
│   ├── GraphSidePanel.tsx             # Detail panel
│   └── GraphHoverCard.tsx             # Hover tooltip
├── layout/
│   ├── communityLayout.ts             # Community anchor computation
│   ├── nodeLayout.ts                  # Force simulation, node positions
│   ├── depthComputation.ts            # Z-depth logic
│   ├── edgeVisibility.ts              # Sparse edge selection
│   └── labelPriority.ts               # Label rendering priority
├── rendering/
│   ├── materials.ts                   # Three.js materials/shaders
│   ├── geometries.ts                  # Node/edge geometries
│   └── renderer.ts                    # Custom rendering logic if needed
├── data/
│   ├── adapter.ts                     # GraphDataAdapter
│   ├── normalize.ts                   # Score normalization
│   └── explain.ts                     # Explainability utility
├── state/
│   ├── graphStore.ts                  # Zustand or equivalent
│   └── graphActions.ts                # Reducers/actions
├── types.ts                           # All TypeScript interfaces
├── constants.ts                       # Colors, layout params, etc.
└── hooks/
    ├── useGraphData.ts                # Fetch and cache graph data
    ├── useGraphLayout.ts              # Compute layout on data change
    └── useGraphInteraction.ts         # Handle clicks/hover/selection
```

(Adjust paths to match existing Continuum patterns if different.)

### 12.2 Component Responsibilities

- **KnowledgeGraph3D:** Stateful wrapper, owns mode, focus, selection
- **GraphScene:** ThreeJS scene, camera, controls, lighting
- **GraphNodes:** Rendered nodes (instanced or meshes)
- **GraphEdges:** Sparse edge rendering
- **GraphLabels:** Label priority + rendering (canvas or Drei Html)
- **GraphControls:** UI buttons/toggles
- **GraphSidePanel:** Selected node details and actions
- **Adapter:** Pure data transformation (no side effects)
- **Layout modules:** Pure computation (no side effects)

---

## 13. Testing

### 13.1 Unit Tests (Priority)

Test pure functions:
- Community anchor derivation
- Edge derivation logic
- Score normalization
- Visible edge top-K selection
- Label priority computation
- Z-depth computation
- Deterministic layout stability

### 13.2 Integration Tests (Medium Priority)

- GraphDataAdapter fetch + transform
- Mode switching (Context ↔ Atlas)
- Node selection → side panel update
- Search query → relevance score recompute
- Empty/error state handling

### 13.3 Visual Tests (Low Priority)

Do **not** over-invest in brittle snapshot tests. Focus on layout correctness instead.

---

## 14. Acceptance Criteria

1. ✅ User can open graph and switch between Atlas Mode and Context Mode
2. ✅ Communities appear as stable 3D orbital regions (not random clouds)
3. ✅ Nodes within communities cluster organically via local force simulation
4. ✅ Search/focus pulls relevant communities forward and fades unrelated ones
5. ✅ Labels are disciplined and do not clutter the graph
6. ✅ Edges are sparse by default and progressively revealed
7. ✅ Clicking a node opens side panel with metadata, summary, relationships, actions
8. ✅ Hover/click interactions explain why nodes/edges appear
9. ✅ Graph remains smooth on realistic data (200+ nodes)
10. ✅ Implementation is typed, modular, matches Continuum architecture
11. ✅ No hardcoded demo data in production paths
12. ✅ No privacy violations; raw evidence hidden by default
13. ✅ No excessive dependencies; bundle impact acceptable
14. ✅ Typecheck, lint, build, and tests pass

---

## 15. Implementation Sequence (Two Phases)

### Phase 1: Backend Graph Projection (Rust/Tauri)

Backend must be built first. Frontend depends on it.

1. **Inspect existing schema** — Review memory/search/storage data structures
2. **Define shared GraphData contract** — Rust types + TypeScript interfaces (bidirectional)
3. **Implement graph projection module** (Rust):
   - Community derivation from project/topic/activity/app/inferred fallback
   - Edge derivation from relationships, temporal proximity, explicit references
   - Score computation (importance, confidence)
   - Privacy filtering (no raw screenshots/OCR in graph nodes)
   - Edge capping (top-K per node, total limits)
4. **Implement Tauri commands:**
   - `get_memory_graph_atlas(params) -> GraphData`
   - `get_memory_graph_context(focus, params) -> GraphData`
   - `get_graph_node_neighborhood(node_id, depth, edge_types) -> GraphData`
   - `get_graph_communities() -> Vec<GraphCommunity>`
   - `build_agent_context_from_graph_selection(node_ids) -> AgentContextPack` (optional)
5. **Test backend projection:**
   - Deterministic community assignments
   - Edge derivation logic
   - Score normalization
   - Privacy filtering
   - Performance on realistic memory volumes
6. **Document graph command responses** — Examples, expected data shapes

### Phase 2: Frontend ThreeJS Rendering (React/TypeScript)

Frontend implementation depends on Phase 1 completion.

1. **Inspect Continuum frontend structure** — Review existing graph/search/memory UI components
2. **Design tokens & visual patterns** — Identify color tokens, layout conventions
3. **Type definitions** — Copy/generate TypeScript interfaces from Rust contract
4. **GraphDataAdapter** — Wire to Tauri graph commands (primary path); add fallback-only memory card derivation
5. **Layout engine:**
   - Deterministic community anchor computation
   - Local force simulation for nodes within communities
   - Z-depth computation (relevance primary, importance/recency modifiers)
6. **ThreeJS scene setup:**
   - Scene, camera, lights, fog, WebGL renderer
   - Orbit controls (pan, rotate, zoom)
   - Node/edge/label geometries
7. **Rendering pipeline:**
   - Instanced node rendering
   - Sparse edge rendering (top-K)
   - Label priority system
   - LOD/culling for performance
8. **Interaction layer:**
   - Click/hover/focus handlers
   - Node selection state
   - Modal switching (Context ↔ Atlas)
9. **Context Mode & Atlas Mode:**
   - Mode-specific layouts and camera behavior
   - Relevance-driven movement (Context Mode)
   - Stable browsing (Atlas Mode)
10. **Side panel:**
    - Selected node details and metadata
    - Related nodes and edge explanations
    - Evidence/provenance expand section
    - Agent actions (context pack, send to agent, etc.)
11. **UI controls:**
    - Mode switch button
    - Community/type/edge-type filters
    - Reset camera, return to center buttons
    - Search integration
12. **Hover cards & explainability:**
    - Hover card with title, app, reason, timestamp
    - Explanations for why nodes appear
    - Progressive disclosure of details
13. **Label discipline:**
    - Label priority computation
    - Zoom-aware label rendering
    - Truncation and avoidance of clutter
14. **Performance & LOD:**
    - Node aggregation when zoomed out
    - Edge culling and top-K selection
    - Evidence nodes hidden by default
    - Memoized layout, debounced updates
15. **Unit tests:**
    - Layout computation correctness
    - Label priority logic
    - Z-depth computation
    - Deterministic stability
16. **Integration tests:**
    - GraphDataAdapter fetch + transform
    - Mode switching behavior
    - Node selection → side panel update
    - Empty/error state handling
17. **Polish & cleanup:**
    - Remove unused code, verify component structure
    - Confirm Continuum aesthetic (dark, cinematic, no generic neon)
    - Document architecture briefly
18. **Final validation:**
    - Typecheck, lint, build pass
    - Unit/integration tests pass
    - Demo with real Continuum data (atlas + context modes)
    - Verify performance targets (60 FPS typical, 30+ FPS large graphs)

---

## 16. Important Constraints

- ❌ Do NOT hardcode a polished fake graph
- ❌ Do NOT overfit to reference images
- ❌ Do NOT create a chaotic full 3D force graph as default
- ❌ Do NOT flood UI with text
- ❌ Do NOT render all edges at once
- ❌ Do NOT expose raw evidence by default
- ❌ Do NOT add cloud dependencies
- ❌ Do NOT create schema migrations unless absolutely required
- ❌ Do NOT break existing search/memory functionality
- ❌ Do NOT make ThreeJS depend on raw OCR, screenshots, or frontend-only memory card text
- ✅ DO prefer small, reversible changes
- ✅ DO keep implementation modular and testable
- ✅ DO wire to real Continuum data, not fixtures
- ✅ DO have backend graph projection produce graph-ready nodes, edges, communities, scores, reasons
- ✅ DO respect backend graph contract; frontend receives clean GraphData

---

## 17. Deliverable

A working 3D graph implementation, or a clearly staged partial implementation if backend integration is incomplete. If partial, use clean TODOs only at integration seams (in adapter layer), not scattered through rendering code.

Include:
- Summary of changed files
- Commands to run
- Any remaining gaps
- Instructions for testing with real Continuum data

---

**End of Specification**
