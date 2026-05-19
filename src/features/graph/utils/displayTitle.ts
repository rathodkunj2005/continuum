import type { GraphNode } from "../types"

export function isIdLikeTitle(title: string): boolean {
  if (!title) return true

  // Check for patterns that look like IDs
  if (title.startsWith("memory ") || title.startsWith("mem_")) return true
  if (title.toLowerCase().startsWith("entity ") || title.toLowerCase().startsWith("ent_")) return true
  if (title.match(/^[a-f0-9]{8,}/i)) return true // Hex hash
  if (title.match(/^[a-f0-9]{8}-[a-f0-9]{4}/i)) return true // UUID-like
  if (title.length > 20 && title.match(/[a-z0-9]{20,}/i)) return true // Long random string

  return false
}

export function getNodeDisplayTitle(node: GraphNode): string {
  // Rule 1: Use title if it's meaningful and not ID-like
  if (node.title && !isIdLikeTitle(node.title)) {
    return node.title.slice(0, 40)
  }

  // Rule 2: Use first sentence of summary
  if (node.summary) {
    const firstSentence = node.summary.split(/[.!?]+/)[0].trim()
    if (firstSentence && !isIdLikeTitle(firstSentence)) {
      return firstSentence.slice(0, 40)
    }
  }

  // Rule 3: Use project/topic
  if (node.project) return node.project.slice(0, 30)
  if (node.topic) return node.topic.slice(0, 30)

  // Rule 4: Use app + window title
  if (node.app_name && node.window_title) {
    return `${node.app_name}: ${node.window_title}`.slice(0, 40)
  }
  if (node.app_name) return node.app_name.slice(0, 30)

  // Rule 5: Use URL hostname or file name
  if (node.url) {
    try {
      const url = new URL(node.url)
      return url.hostname || url.pathname
    } catch {
      // Fallthrough
    }
  }

  // Rule 6: Activity type for entities
  if (node.activity_type) return node.activity_type.slice(0, 30)

  // Final fallback: do NOT use ID
  return "Untitled"
}
