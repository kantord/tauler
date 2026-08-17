// Every Tailwind utility the components actually render must have a rule in the compiled
// stylesheet.
//
// This is not a style check. A class with no rule is one of two things, and both matter:
// a utility takumi supports and real Tailwind does not (the web render would differ from
// the takumi one), or a theme token nothing resolves (both renders are wrong, quietly).
// The second is how `text-destructive-foreground` was found.

import { readFileSync } from 'node:fs'

const CSS = 'public/tauler/tauler.css'
const CLASSES = '.tauler/classes.txt'

/**
 * Classes known to have no rule, with the reason. An entry here is a bug that is filed,
 * not a class that is fine — the list should shrink.
 */
const KNOWN_UNRESOLVED = new Map([
  [
    'text-destructive-foreground',
    '`destructive-foreground` is not in themes/default.yaml, so nothing resolves it — ' +
      '<Badge variant="destructive"> renders with an inherited text colour on both ' +
      'renderers. Pre-existing; surfaced by the class harvest.',
  ],
])

const css = readFileSync(CSS, 'utf8')
const classes = readFileSync(CLASSES, 'utf8')
  .split('\n')
  .map((l) => l.trim())
  .filter(Boolean)

const selectors = new Set(
  [...css.matchAll(/\.((?:\\.|[A-Za-z0-9_-])+)/g)].map((m) => m[1].replace(/\\(.)/g, '$1')),
)

const missing = classes.filter((c) => !selectors.has(c))
const unexpected = missing.filter((c) => !KNOWN_UNRESOLVED.has(c))
const stale = [...KNOWN_UNRESOLVED.keys()].filter((c) => !missing.includes(c))

for (const c of missing.filter((c) => KNOWN_UNRESOLVED.has(c))) {
  console.warn(`known unresolved: ${c}\n  ${KNOWN_UNRESOLVED.get(c)}`)
}
for (const c of stale) {
  console.error(`stale allowlist entry: ${c} now resolves — remove it from KNOWN_UNRESOLVED`)
}
for (const c of unexpected) {
  console.error(`no rule compiled for: ${c}`)
}

if (unexpected.length || stale.length) {
  console.error(
    `\n${classes.length} classes harvested, ${unexpected.length} without a rule, ` +
      `${stale.length} stale allowlist entries.`,
  )
  process.exit(1)
}
console.log(`${classes.length} classes harvested, all resolved or accounted for.`)
