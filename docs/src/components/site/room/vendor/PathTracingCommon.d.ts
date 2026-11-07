/**
 * Registers the renderer's GLSL chunks on `THREE.ShaderChunk`. Takes only
 * `{ ShaderChunk }`: passing the whole three namespace keeps all of three in
 * the bundle.
 */
export function installPathTracingChunks(THREE: {
  ShaderChunk: Record<string, string>
}): void
