// Class-layer enforcement for the site's atomic-ish design system.
//
// Layers (see ADR 0030):
//   src/pages/**              may only lay out: flex/grid, gap, sizing, position.
//   src/components/text/**    own typography and text color.
//   src/components/ui/**      own surfaces, hairlines, spacing — and their text.
//   src/components/organisms/** compose atoms; layout plus section chrome
//                             (hairline borders, surface backgrounds).
// Marker classes (`above-fold`) are metadata, allowed everywhere. `crt-*`
// classes are the same kind of thing: hooks into crt.css (a separate,
// deliberately-unlayered stylesheet scoped to the landing page's CRT
// overlay — see that file's own top comment), not Tailwind utilities, so
// they're not subject to the layer rules below.
// `rounded-*`, blur and backdrop utilities are banned everywhere: radius 0
// and "never blur what you cover" are design invariants.

import { readdirSync, readFileSync } from 'node:fs'
import { join, relative } from 'node:path'

const ROOT = new URL('..', import.meta.url).pathname

const LAYOUT =
  /^(flex|inline-flex|grid|block|inline|hidden|items-|justify-|content-|self-|gap(-|$)|p[trblxye]?-|m[trblxye]?-|w-|h-|min-w-|min-h-|max-w-|max-h-|size-|absolute|relative|fixed|sticky|static|inset-|top-|right-|bottom-|left-|z-|overflow-|shrink|grow|basis-|order-|col-|row-|leading-none)/

const TYPOGRAPHY =
  /^(font-|text-|tracking-|uppercase|lowercase|capitalize|italic|underline|no-underline|whitespace-|break-|truncate)/

const SURFACE =
  /^(bg-|border(-|$)|shadow-|outline|ring|cursor-|select-|opacity-|brightness-|transition|duration-|ease-)/

const MARKERS = /^(above-fold|crt-[\w-]+)$/

const BANNED = /^(rounded|blur|backdrop-)/

const LAYER_RULES: { dir: string; allowed: RegExp[]; label: string }[] = [
  {
    dir: 'src/pages',
    label: 'pages compose with layout classes only',
    allowed: [LAYOUT, MARKERS],
  },
  {
    dir: 'src/components/text',
    label: 'text atoms own typography and text color',
    allowed: [TYPOGRAPHY, MARKERS, /^(max-w-measure|block|inline)$/],
  },
  {
    dir: 'src/components/ui',
    label: 'ui atoms own surfaces, spacing and their own text',
    allowed: [TYPOGRAPHY, SURFACE, LAYOUT, MARKERS],
  },
  {
    dir: 'src/components/organisms',
    label: 'organisms lay out atoms; chrome limited to hairlines and surfaces',
    allowed: [LAYOUT, MARKERS, /^border(-|$)/, /^bg-(surface|line)-/],
  },
]

function astroFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true, recursive: true })
    .filter((e) => e.isFile() && e.name.endsWith('.astro'))
    .map((e) => join(e.parentPath, e.name))
}

function classesIn(rawSource: string): string[] {
  // class="..." attributes plus string literals inside class:list={[...]}.
  // Template literals hold sample code shown to visitors (e.g. DemoColumn's
  // syntax-highlighted snippet) — illustrative text, not our own markup —
  // and can legitimately contain the literal substring `class="..."`;
  // strip them before scanning so that text is never mistaken for a real
  // attribute on this file's own elements.
  const source = rawSource.replace(/`[\s\S]*?`/g, '')
  const out: string[] = []
  for (const m of source.matchAll(/\bclass="([^"]*)"/g))
    out.push(...m[1].split(/\s+/))
  for (const m of source.matchAll(/\bclass:list=\{\[([^]*?)\]\}/g)) {
    for (const lit of m[1].matchAll(/'([^']*)'/g))
      out.push(...lit[1].split(/\s+/))
  }
  return out.filter(Boolean)
}

// Variants (tight:, short:, hover:, max-md:, [&_code]: …) wrap a base class;
// the base class is what the layer rules judge.
function baseClass(cls: string): string {
  return cls.replace(/^([a-z-]+:|\[[^\]]*\]:)+/, '')
}

let failures = 0
for (const rule of LAYER_RULES) {
  for (const file of astroFiles(join(ROOT, rule.dir))) {
    for (const cls of classesIn(readFileSync(file, 'utf8'))) {
      const base = baseClass(cls)
      const rel = relative(ROOT, file)
      if (BANNED.test(base)) {
        console.error(
          `${rel}: "${cls}" is banned everywhere (radius 0 / no blur)`,
        )
        failures++
      } else if (!rule.allowed.some((re) => re.test(base))) {
        console.error(`${rel}: "${cls}" not allowed here — ${rule.label}`)
        failures++
      }
    }
  }
}

if (failures > 0) {
  console.error(`\n${failures} class-layer violation(s).`)
  process.exit(1)
}
console.log('class layers OK')
