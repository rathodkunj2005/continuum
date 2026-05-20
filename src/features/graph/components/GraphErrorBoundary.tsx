import React, { ReactNode } from "react"

interface Props {
  children: ReactNode
  onReset: () => void
  fallbackMessage?: string
}

interface State {
  hasError: boolean
  error: Error | null
}

export class GraphErrorBoundary extends React.Component<Props, State> {
  constructor(props: Props) {
    super(props)
    this.state = { hasError: false, error: null }
  }

  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error) {
    console.error("[GraphErrorBoundary] caught error:", error)
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex items-center justify-center w-full h-full bg-slate-900 rounded-lg">
          <div className="text-center">
            <p className="text-red-400 mb-4">Graph rendering failed</p>
            <p className="text-sm text-slate-400">
              {this.props.fallbackMessage ||
                "An error occurred while rendering the 3D graph."}
            </p>
            <button
              onClick={this.props.onReset}
              className="mt-4 px-4 py-2 bg-slate-700 hover:bg-slate-600 rounded text-sm"
            >
              Switch back to 2D
            </button>
          </div>
        </div>
      )
    }

    return this.props.children
  }
}
