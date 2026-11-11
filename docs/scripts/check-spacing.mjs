// Spacing lint (docs/DESIGN.md). Padding, margin and gap come from a short
// allowlist; sizing (h-*, size-*, w-*, max-w-*) is left alone on purpose, since
// shadcn components use many sizing steps and restricting that namespace in
// Tailwind (`--spacing-*: initial`) breaks them.
//
//   node scripts/check-spacing.mjs                scan src/ (components/ui exempt)
//   node scripts/check-spacing.mjs --root <dir>   scan another tree, nothing exempt
//   node scripts/check-spacing.mjs --list         print every distinct value found
//
// Opt out on one line with a `spacing-ok: reason` comment.
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

// Tailwind units (1 = 4px). 0.5 is for hairline cases only. 5 and 7 are the
// tile gap and tile padding the design names (space-5, space-7).
const STEPS = new Set([
  '0',
  '0.5',
  '1',
  '2',
  '3',
  '4',
  '5',
  '6',
  '7',
  '8',
  '12',
  '16',
  'px',
  'auto',
])
// px values allowed in CSS padding, margin and gap declarations.
const PX = new Set([0, 1, 2, 4, 8, 12, 16, 20, 24, 28, 32, 48, 64])

const args = process.argv.slice(2)
const rootArg = args.indexOf('--root')
const here = fileURLToPath(new URL('..', import.meta.url))
const root = rootArg >= 0 ? args[rootArg + 1] : join(here, 'src')
const exempt = rootArg >= 0 ? [] : ['components/ui/']

const UTIL =
  /(?<![\w-])((?:[a-z0-9-]+:|\*:|\[[^\]]+\]:)*)(-?)((?:p|px|py|pt|pr|pb|pl|ps|pe|m|mx|my|mt|mr|mb|ml|ms|me|gap|gap-x|gap-y|space-x|space-y)-)(\[[^\]\s]+\]|\([^)\s]+\)|[0-9]+(?:\.[0-9]+)?|px|auto)(?=[\s"'`)]|$)/g

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === 'node_modules' || name.startsWith('.')) continue
    const p = join(dir, name)
    if (statSync(p).isDirectory()) walk(p, out)
    else if (/\.(tsx?|css|mdx?)$/.test(name)) out.push(p)
  }
  return out
}

const found = new Map()
const problems = []
for (const file of walk(root)) {
  const rel = relative(root, file)
  if (exempt.some((e) => rel.startsWith(e))) continue
  const lines = readFileSync(file, 'utf8').split('\n')
  lines.forEach((line, i) => {
    if (line.includes('spacing-ok')) return
    const where = `${relative(process.cwd(), file)}:${i + 1}`
    if (!file.endsWith('.css')) {
      for (const m of line.matchAll(UTIL)) {
        const value = m[4]
        found.set(value, (found.get(value) ?? 0) + 1)
        const isVar = value.startsWith('(') || /var\(|--/.test(value)
        if (!isVar && !STEPS.has(value))
          problems.push(
            `${where}  ${m[3]}${value} is not an allowed spacing step`,
          )
      }
    } else {
      for (const d of line.matchAll(
        /(?:padding|margin|gap)[a-z-]*:\s*([^;]+);/g,
      )) {
        for (const n of d[1].matchAll(/(-?\d+(?:\.\d+)?)px/g)) {
          const px = Math.abs(Number(n[1]))
          found.set(`${px}px`, (found.get(`${px}px`) ?? 0) + 1)
          if (!PX.has(px))
            problems.push(`${where}  ${n[0]} is not an allowed spacing value`)
        }
      }
    }
  })
}

if (args.includes('--list')) {
  for (const [k, v] of [...found].sort(
    (a, b) => parseFloat(a[0]) - parseFloat(b[0]),
  ))
    console.log(`${k.padStart(8)} ${v}`)
}
if (problems.length) {
  console.error(problems.join('\n'))
  console.error(`${problems.length} spacing value(s) outside the allowlist`)
  process.exit(1)
}
console.log('spacing ok')
