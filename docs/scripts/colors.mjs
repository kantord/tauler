// Generates and checks the design system's derived colours (docs/DESIGN.md).
//
//   node scripts/colors.mjs          check (CI): contrast of every text pair in
//                                    src/app/global.css, and that the derived
//                                    tokens still equal what the generator gives
//   node scripts/colors.mjs --print  print the generated values as oklch()
//
// Derived tokens are solved with Adobe's Leonardo against the page colour. Its
// OKLCH output string is malformed in v1.1.0, so we take HEX and convert with
// culori. Leonardo also moves the background it solves against (its `lightness`
// slider), so every generated value is re-measured against the real page here.
// WCAG 2 is the gate; APCA (apca-w3) is printed as advice and never fails.
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { BackgroundColor, Color, Theme } from '@adobe/leonardo-contrast-colors'
import { APCAcontrast, sRGBtoY } from 'apca-w3'
import { clampRgb, oklch, parse, rgb, wcagContrast } from 'culori'

const CSS = fileURLToPath(new URL('../src/app/global.css', import.meta.url))

// The brand anchors the page colours are solved against (hex of global.css).
const PAGE = { light: '#e6e4de', dark: '#111110' }
const LIGHTNESS = { light: 90, dark: 7 }

// token -> theme -> contrast ratio against `page`, and the ramp's key colours.
const RAMPS = {
  neutral: {
    keys: ['#5d5b55', '#111111'],
    tokens: {
      'ink-muted': { light: 6, dark: 6 },
      line: { light: 1.4, dark: 1.4 },
      rule: { dark: 3.4 },
    },
  },
  red: { keys: ['#a61b1b'], tokens: { red: { light: 5.2, dark: 5.6 } } },
  orange: { keys: ['#e2561f'], tokens: { 'orange-ink': { light: 5 } } },
}

function generate(mode) {
  const out = {}
  for (const ramp of Object.values(RAMPS)) {
    const ratios = {}
    for (const [name, r] of Object.entries(ramp.tokens))
      if (r[mode]) ratios[name] = r[mode]
    if (!Object.keys(ratios).length) continue
    const color = new Color({
      name: 'c',
      colorKeys: ramp.keys,
      colorSpace: 'OKLCH',
      ratios,
    })
    const bg = new BackgroundColor({
      name: 'page',
      colorKeys: [PAGE[mode]],
      colorSpace: 'OKLCH',
      ratios: [1],
    })
    const theme = new Theme({
      colors: [bg, color],
      backgroundColor: bg,
      lightness: LIGHTNESS[mode],
      contrast: 1,
      saturation: 100,
      output: 'HEX',
      formula: 'wcag2',
    })
    for (const name of Object.keys(ratios))
      out[name] = theme.contrastColorPairs[name]
  }
  return out
}

const fmt = (c) => {
  const o = oklch(parse(c))
  return `oklch(${o.l.toFixed(3)} ${(o.c || 0).toFixed(3)} ${(o.h ?? 0).toFixed(1)})`
}

if (process.argv.includes('--print')) {
  for (const mode of ['light', 'dark']) {
    console.log(`${mode}:`)
    for (const [k, v] of Object.entries(generate(mode)))
      console.log(`  --${k}: ${fmt(v)};`)
  }
  process.exit(0)
}

// ── read the tokens from global.css ────────────────────────────────────────
const css = readFileSync(CSS, 'utf8')
function block(selector) {
  const m = css.match(
    new RegExp(`(?:^|\\n)${selector}\\s*\\{([\\s\\S]*?)\\n\\}`),
  )
  if (!m) throw new Error(`no ${selector} block in global.css`)
  const vars = {}
  for (const [, k, v] of m[1].matchAll(/--([a-z0-9-]+):\s*([^;]+);/g))
    vars[k] = v.trim()
  return vars
}
const light = block(':root')
const dark = { ...light, ...block('\\.dark') }
function resolve(vars, name, depth = 0) {
  const v = vars[name]
  if (v === undefined) throw new Error(`token --${name} is not defined`)
  const alias = v.match(/^var\(--([a-z0-9-]+)\)$/)
  if (alias) {
    if (depth > 5) throw new Error(`alias loop at --${name}`)
    return resolve(vars, alias[1], depth + 1)
  }
  return v
}
const themes = { light, dark }
const tok = (mode, name) => resolve(themes[mode], name)

let failures = 0
const fail = (msg) => {
  failures++
  console.error(`FAIL ${msg}`)
}

// 1. Derived tokens equal the generator output.
for (const mode of ['light', 'dark']) {
  for (const [name, hex] of Object.entries(generate(mode))) {
    const want = oklch(parse(hex))
    const have = oklch(parse(tok(mode, name)))
    if (
      Math.abs(want.l - have.l) > 0.004 ||
      Math.abs((want.c || 0) - (have.c || 0)) > 0.006
    )
      fail(
        `${mode} --${name} is ${fmt(tok(mode, name))}, the generator gives ${fmt(hex)}`,
      )
  }
}

// 2. Contrast (WCAG 2 gate, APCA advice). `min` is WCAG 2; `lc` is the APCA
// advisory floor (body text 75, content text 60, large or UI 45).
const Y = (c) => {
  const x = clampRgb(rgb(parse(c)))
  return sRGBtoY([x.r * 255, x.g * 255, x.b * 255].map(Math.round))
}
const PAIRS = [
  // [foreground, ground, min WCAG 2, APCA advisory Lc]
  ['ink', 'page', 7, 75],
  ['ink', 'raised', 7, 75],
  ['ink-muted', 'page', 4.5, 60],
  ['ink-muted', 'raised', 4.5, 60],
  ['red', 'page', 4.5, 60],
  ['red', 'raised', 4.5, 60],
  ['orange-ink', 'page', 4.5, 60],
  ['orange-ink', 'raised', 4.5, 60],
  ['rule', 'page', 3, 45],
  ['rule', 'raised', 3, 45],
  ['on-orange', 'orange', 4.5, 75],
  ['code-ink', 'code', 7, 75],
  ['code-soft', 'code', 4.5, 60],
  ['code-muted', 'code', 4.5, 60],
  ['code-orange', 'code', 4.5, 60],
  ['code-orange-soft', 'code', 4.5, 60],
  ['code-yellow', 'code', 4.5, 60],
]
// `orange` on the light page is 2.96:1: it is only ever a fill (DESIGN.md), so
// it is deliberately not a pair here. Text and focus rings use orange-ink.
const advice = []
for (const mode of ['light', 'dark']) {
  for (const [fg, bg, min, lc] of PAIRS) {
    const f = tok(mode, fg)
    const b = tok(mode, bg)
    const ratio = wcagContrast(parse(f), parse(b))
    if (ratio < min)
      fail(`${mode} ${fg} on ${bg}: ${ratio.toFixed(2)}:1, needs ${min}:1`)
    const apca = Math.abs(APCAcontrast(Y(f), Y(b)))
    if (apca < lc)
      advice.push(
        `${mode} ${fg} on ${bg}: APCA Lc ${apca.toFixed(0)}, below the advisory ${lc} (WCAG 2 ${ratio.toFixed(2)}:1)`,
      )
  }
}
for (const a of advice) console.warn(`advice ${a}`)

if (failures) {
  console.error(`${failures} colour check(s) failed`)
  process.exit(1)
}
console.log(
  `colours ok: ${PAIRS.length * 2} contrast pairs and the derived tokens match` +
    (advice.length ? `; ${advice.length} APCA advisories` : ''),
)
