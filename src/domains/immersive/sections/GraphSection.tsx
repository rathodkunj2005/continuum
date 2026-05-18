import { useEffect, useState } from "react";
import { StickyScene } from "@/domains/immersive/components/StickyScene";
import { useSetWallpaperPage } from "@/shared/hooks/useImmersiveWallpaper";
import { KnowledgeGraph } from "@/domains/memory-vault/KnowledgeGraph";
import { getFullGraph, type InsightGraphNode, type InsightGraphEdge } from "@/shared/ipc/tauri";
import "./GraphSection.css";

const SECTION_ID = "graph";

/**
 * Slice 6 — Memory graph scene.
 * Wraps the full KnowledgeGraph canvas in a sticky scroll budget.
 */
export function GraphSection() {
    useSetWallpaperPage("graph", SECTION_ID);

    const [nodes, setNodes] = useState<InsightGraphNode[]>([]);
    const [edges, setEdges] = useState<InsightGraphEdge[]>([]);

    useEffect(() => {
        let cancelled = false;
        void getFullGraph().then((g) => {
            if (!cancelled) {
                setNodes(g.nodes);
                setEdges(g.edges);
            }
        });
        return () => { cancelled = true; };
    }, []);

    return (
        <StickyScene sectionId={SECTION_ID} scrollBudget={1200}>
            <div className="fndr-graph-scene">
                <div className="fndr-graph-scene-header">
                    <p className="fndr-graph-scene-label">Memory graph</p>
                    <h2 className="fndr-graph-scene-heading">
                        your knowledge, visualised
                    </h2>
                </div>
                <div className="fndr-graph-scene-canvas">
                    <KnowledgeGraph nodes={nodes} edges={edges} height={520} />
                </div>
            </div>
        </StickyScene>
    );
}

export default GraphSection;
