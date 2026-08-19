// Every Tailwind utility the components render must have a rule in the compiled stylesheet.
//
// Not a style check. A class with no rule is either a utility takumi supports and Tailwind
// does not — the two renderers would differ — or a theme token nothing resolves, which is
// wrong on both. The second is how `text-destructive-foreground` was found.

import { readFileSync } from 'node:fs'

/** Known to have no rule, with the reason. Entries are filed bugs; the list should shrink. */
const KNOWN_UNRESOLVED = new Map([])

const css = readFileSync('public/tauler/tauler.css', 'utf8')
const classes = readFileSync('.tauler/classes.txt', 'utf8')
  .split('\n')
  .map((l) => l.trim())
  .filter(Boolean)
const selectors = new Set(
  [...css.matchAll(/\.((?:\\.|[A-Za-z0-9_-])+)/g)].map((m) =>
    m[1].replace(/\\(.)/g, '$1'),
  ),
)

const missing = classes.filter((c) => !selectors.has(c))
const unexpected = missing.filter((c) => !KNOWN_UNRESOLVED.has(c))
const stale = [...KNOWN_UNRESOLVED.keys()].filter((c) => !missing.includes(c))

for (const c of missing.filter((c) => KNOWN_UNRESOLVED.has(c))) {
  console.warn(`known unresolved: ${c} — ${KNOWN_UNRESOLVED.get(c)}`)
}
for (const c of stale)
  console.error(`${c} now resolves — remove it from KNOWN_UNRESOLVED`)
for (const c of unexpected) console.error(`no rule compiled for: ${c}`)

if (unexpected.length || stale.length) process.exit(1)
console.log(
  `${classes.length} classes harvested, all resolved or accounted for.`,
)
