// A Turbopack loader (next.config.mjs, turbopack.rules) that drops the comments from the room's
// GLSL in what ships: a `.glsl` file (imported as text) or the GLSL template literals of
// room/vendor/PathTracingCommon.js. The source files keep their comments, provenance headers
// included; about a third of the renderer's GLSL is comments.
//
// It removes `//` and `/* */` comments (a block comment that spans lines becomes a line break,
// one inside a line a space, so no tokens or preprocessor lines join), trailing whitespace,
// empty lines and carriage returns (the vendored file has Windows line endings). Indentation
// stays. The room's load-time edits (room-shader.ts) run on the result, so their anchors are
// code, not comments; a missed anchor throws.
//
// CommonJS because Turbopack runs webpack loaders through require().

/** GLSL with its comments, trailing whitespace and empty lines removed. */
function stripGlsl(glsl) {
  return glsl
    .replace(/\r\n/g, '\n')
    .replace(/\/\*[\s\S]*?\*\/|\/\/[^\n]*/g, (c) =>
      c.startsWith('/*') ? (c.includes('\n') ? '\n' : ' ') : '',
    )
    .replace(/[ \t]+$/gm, '')
    .replace(/\n{2,}/g, '\n')
    .replace(/^\n/, '')
}

module.exports = function stripGlslComments(source) {
  if (this.resourcePath.endsWith('.glsl')) return stripGlsl(source)
  // PathTracingCommon.js: every chunk is a template literal with no substitutions
  return source.replace(/`([^`]*)`/g, (_, glsl) => `\`${stripGlsl(glsl)}\``)
}
module.exports.stripGlsl = stripGlsl
