// The docs only use container directives (`:::note`, `:::caution`, …). But
// remark-directive also parses text (`:name`) and leaf (`::name`) directives,
// and in prose those only ever come from ratios and times: "3:1", "16:9",
// "10:30". Left alone they render as a <div> inside a <p> — invalid nesting
// and a hydration error. Run right after remark-directive to turn them back
// into the text they were written as.
//
// Walks the tree by hand (no unist-util-visit) and types nodes locally, so it
// needs no extra direct dependency.

interface Node {
  type: string
  value?: string
  name?: string
  attributes?: Record<string, string | null | undefined> | null
  children?: Node[]
}

const text = (value: string): Node => ({ type: 'text', value })

// `{#id .class key="value"}` from the parsed attributes; empty when none.
function attributeSource(attributes: Node['attributes']): string {
  const entries = Object.entries(attributes ?? {})
  if (entries.length === 0) return ''
  const parts = entries.map(([key, value]) =>
    value == null || value === '' ? key : `${key}="${value}"`,
  )
  return `{${parts.join(' ')}}`
}

// The directive as the phrasing it came from: `:name[label]{attrs}`, with the
// label's own (already parsed) children kept as they are.
function literal(node: Node, colons: string): Node[] {
  const label = node.children ?? []
  return [
    text(`${colons}${node.name ?? ''}${label.length > 0 ? '[' : ''}`),
    ...label,
    text(`${label.length > 0 ? ']' : ''}${attributeSource(node.attributes)}`),
  ].filter((child) => child.type !== 'text' || child.value !== '')
}

function restore(parent: Node): void {
  if (!parent.children) return
  parent.children = parent.children.flatMap((child) => {
    restore(child)
    if (child.type === 'textDirective') return literal(child, ':')
    if (child.type === 'leafDirective')
      return [{ type: 'paragraph', children: literal(child, '::') }]
    return [child]
  })
}

export function remarkLiteralDirectives() {
  return (tree: Node) => restore(tree)
}
