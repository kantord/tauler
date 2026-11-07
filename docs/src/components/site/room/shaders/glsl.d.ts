/** A .glsl file, imported as its text without comments (scripts/strip-glsl-comments.cjs). */
declare module '*.glsl' {
  const glsl: string
  export default glsl
}
