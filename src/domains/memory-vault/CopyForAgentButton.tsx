import { useState } from "react";
import { continuumBuildContextPack } from "../../shared/ipc/tauri";

interface Props {
    query: string;
    project?: string;
}

/**
 * Phase 5 — "Copy for Agent" button. Calls continuumBuildContextPack and writes
 * a markdown-rendered ContextPack to the clipboard. Shows a transient
 * "Copied" toast so the user knows the action succeeded.
 */
export function CopyForAgentButton({ query, project }: Props) {
    const [status, setStatus] = useState<"idle" | "copying" | "copied" | "error">("idle");

    async function handleClick() {
        setStatus("copying");
        try {
            const pack = await continuumBuildContextPack({ query, project });
            const md = renderContextPackMarkdown(pack);
            await navigator.clipboard.writeText(md);
            setStatus("copied");
            setTimeout(() => setStatus("idle"), 1800);
        } catch (err) {
            console.error("CopyForAgent failed", err);
            setStatus("error");
            setTimeout(() => setStatus("idle"), 2400);
        }
    }

    return (
        <button
            type="button"
            onClick={handleClick}
            disabled={status === "copying"}
            data-testid="continuum-copy-for-agent"
            style={{
                padding: "8px 14px",
                background: status === "copied" ? "#388E3C" : "#3E2723",
                color: "#FAF9F6",
                borderRadius: 8,
                border: "none",
                fontSize: 13,
                cursor: status === "copying" ? "wait" : "pointer",
            }}
        >
            {status === "copying"
                ? "Copying…"
                : status === "copied"
                  ? "Copied!"
                  : status === "error"
                    ? "Copy failed"
                    : "Copy for Agent"}
        </button>
    );
}

function renderContextPackMarkdown(pack: unknown): string {
    if (typeof pack !== "object" || pack === null) return String(pack);
    const p = pack as Record<string, unknown>;
    const lines: string[] = [];
    if (typeof p.query === "string") lines.push(`# Continuum context: ${p.query}`);
    if (typeof p.summary === "string" && p.summary.trim().length > 0) {
        lines.push("", p.summary);
    }
    if (Array.isArray(p.relevant_files) && p.relevant_files.length > 0) {
        lines.push("", "## Relevant files");
        for (const f of p.relevant_files) {
            const path = (f as { path?: string }).path;
            if (path) lines.push(`- ${path}`);
        }
    }
    if (Array.isArray(p.recent_decisions) && p.recent_decisions.length > 0) {
        lines.push("", "## Recent decisions");
        for (const d of p.recent_decisions) {
            const summary = (d as { summary?: string }).summary;
            if (summary) lines.push(`- ${summary}`);
        }
    }
    return lines.join("\n");
}
