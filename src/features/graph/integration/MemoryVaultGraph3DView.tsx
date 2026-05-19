import React from "react"
import { KnowledgeGraph3D } from "../components/KnowledgeGraph3D"

interface MemoryVaultGraph3DViewProps {
  isVisible: boolean
  onClose: () => void
}

/**
 * Integration component for the 3D graph in the memory vault.
 * This component is shown in place of the 2D graph when the 3D toggle is active.
 */
export const MemoryVaultGraph3DView: React.FC<MemoryVaultGraph3DViewProps> = ({
  isVisible,
  onClose,
}) => {
  if (!isVisible) return null

  return (
    <div className="memory-graph-layout">
      <div className="memory-graph-stage" style={{ height: "100%" }}>
        <KnowledgeGraph3D onClose={onClose} />
      </div>
    </div>
  )
}
