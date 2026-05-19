import * as THREE from "three"

// Cache geometries to avoid recreating them
const geometryCache = new Map<string, THREE.BufferGeometry>()

export function getNodeGeometry(size: number): THREE.SphereGeometry {
  const cacheKey = `sphere-${size.toFixed(2)}`

  if (!geometryCache.has(cacheKey)) {
    // Scale-dependent geometry: larger nodes can have more segments
    const segments = Math.max(8, Math.min(32, Math.round(size * 8)))
    const geometry = new THREE.SphereGeometry(size, segments, segments)
    geometryCache.set(cacheKey, geometry)
  }

  return geometryCache.get(cacheKey) as THREE.SphereGeometry
}

export function getEdgeGeometry(): THREE.BufferGeometry {
  const cacheKey = "edge-line"

  if (!geometryCache.has(cacheKey)) {
    const geometry = new THREE.BufferGeometry()
    // Edges will be created dynamically per edge
    geometryCache.set(cacheKey, geometry)
  }

  return geometryCache.get(cacheKey) as THREE.BufferGeometry
}

export function getCommunityAnchorGeometry(size: number): THREE.OctahedronGeometry {
  const cacheKey = `octahedron-${size.toFixed(2)}`

  if (!geometryCache.has(cacheKey)) {
    const geometry = new THREE.OctahedronGeometry(size, 2)
    geometryCache.set(cacheKey, geometry)
  }

  return geometryCache.get(cacheKey) as THREE.OctahedronGeometry
}

export function getLabelPlaneGeometry(width: number = 1, height: number = 0.25): THREE.PlaneGeometry {
  const cacheKey = `plane-${width}-${height}`

  if (!geometryCache.has(cacheKey)) {
    const geometry = new THREE.PlaneGeometry(width, height)
    geometryCache.set(cacheKey, geometry)
  }

  return geometryCache.get(cacheKey) as THREE.PlaneGeometry
}

export function createEdgeLineSegments(
  source: { x: number; y: number; z: number },
  target: { x: number; y: number; z: number }
): THREE.BufferGeometry {
  const geometry = new THREE.BufferGeometry()

  const positions = new Float32Array([source.x, source.y, source.z, target.x, target.y, target.z])

  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3))

  return geometry
}

export function createCurveBetweenPoints(
  source: { x: number; y: number; z: number },
  target: { x: number; y: number; z: number },
  controlPoint?: { x: number; y: number; z: number }
): THREE.BufferGeometry {
  // Bezier curve from source to target with optional control point
  const curve = new THREE.CatmullRomCurve3([
    new THREE.Vector3(source.x, source.y, source.z),
    new THREE.Vector3(
      controlPoint?.x ?? (source.x + target.x) / 2,
      controlPoint?.y ?? (source.y + target.y) / 2,
      controlPoint?.z ?? (source.z + target.z) / 2
    ),
    new THREE.Vector3(target.x, target.y, target.z),
  ])

  return new THREE.BufferGeometry().setFromPoints(curve.getPoints(16))
}

export function clearGeometryCache(): void {
  geometryCache.forEach((geometry) => {
    geometry.dispose()
  })
  geometryCache.clear()
}

export function getGeometryCacheSize(): number {
  return geometryCache.size
}

export function disposeGeometry(geometry: THREE.BufferGeometry): void {
  geometry.dispose()
}
