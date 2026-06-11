import type { PanelKey } from "@/domains/command-palette/CommandPalette";

export interface AppToast {
    id: string;
    title: string;
    body: string;
    kind: string;
    actionLabel?: string;
    targetPanel?: PanelKey;
    /** When set with targetPanel "memoryCards", the vault opens focused on this memory. */
    memoryId?: string;
}
