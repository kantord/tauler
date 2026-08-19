// Hidden tuning panel for the CRT overlay (crt.css). Never linked from the
// page — reachable only by typing `crt.tune()` in the devtools console.
// Every effect layer already reads its strength from a CSS custom
// property with a shipped default as the var() fallback; this panel does
// nothing but set those same properties as inline styles on <html>, which
// beats the stylesheet default without editing a single rule.
//
// Knobs are grouped under LAYERS, each with its own on/off toggle
// (html[data-crt-hide~="id"], see crt.css) — sliders alone don't let you
// isolate one effect from the rest, and with six-plus animated layers
// stacked on top of each other it's genuinely hard to tell which one is
// doing what you're looking at. Toggle everything else off to check one
// layer in isolation.

import { setPageWarp, setPageWarpStrength } from './crt-warp'
import { setTextAttenuation } from './crt-text-mask'
import { setCanvasFringe } from './crt-canvas-fringe'

type Knob = {
  prop: string
  label: string
  hint: string
  min: number
  max: number
  step: number
  default: number
  unit: string
}

type Layer = {
  id: string
  label: string
  hint: string
  knobs: Knob[]
}

const LAYERS: Layer[] = [
  {
    id: 'scanlines',
    label: 'Scanlines',
    hint: 'Fine horizontal line mesh over the whole page, bowed into a barrel curve top and bottom.',
    knobs: [
      {
        prop: '--crt-scan-alpha',
        label: 'Darkness',
        hint: '0 = invisible. Near max = dark venetian blinds.',
        min: 0,
        max: 0.6,
        step: 0.005,
        default: 0.42,
        unit: '',
      },
      {
        prop: '--crt-scan-flicker-floor',
        label: 'Flicker floor',
        hint: 'How dark the scanlines dip on each pulse, as a fraction of full darkness. Lower = more noticeable pulse. 1 = no pulse.',
        min: 0.3,
        max: 1,
        step: 0.01,
        default: 0.7,
        unit: '',
      },
      {
        prop: '--crt-scan-flicker-duration',
        label: 'Flicker period (s)',
        hint: 'How long one full darken-and-recover pulse takes.',
        min: 0.5,
        max: 12,
        step: 0.1,
        default: 3.6,
        unit: 's',
      },
      {
        prop: '--crt-scan-drift',
        label: 'Drift amount (px)',
        hint: "The scanline mesh's slow vertical wobble — not the flicker, actual up/down motion. 0 = perfectly still.",
        min: 0,
        max: 20,
        step: 0.5,
        default: 4,
        unit: 'px',
      },
      {
        prop: '--crt-scan-drift-duration',
        label: 'Drift period (s)',
        hint: 'How long one full up-down-up drift cycle takes.',
        min: 1,
        max: 20,
        step: 0.5,
        default: 20,
        unit: 's',
      },
      {
        prop: '--crt-bow',
        label: 'Bow radius',
        hint: "Smaller = stronger barrel curve, sagging toward vertical centre. Larger = flatter, near-straight. If this still looks wrong, toggle every OTHER layer off first — with everything stacked at once it's easy to mistake beam/grain/aberration motion for a scanline problem.",
        min: 150,
        max: 1200,
        step: 10,
        default: 380,
        unit: 'vh',
      },
    ],
  },
  {
    id: 'vignette',
    label: 'Vignette',
    hint: 'Static radial darkening of the four corners. No animation, nothing to watch for over time.',
    knobs: [
      {
        prop: '--crt-vignette-alpha',
        label: 'Strength',
        hint: '0 = none. Near max = corners go nearly black.',
        min: 0,
        max: 0.8,
        step: 0.01,
        default: 0.37,
        unit: '',
      },
    ],
  },
  {
    id: 'glow',
    label: 'Edge glow',
    hint: 'Thin accent-coloured line hugging the true top and bottom edge of the viewport — not the hero, the whole browser window edge.',
    knobs: [
      {
        prop: '--crt-glow-base',
        label: 'Resting',
        hint: '0 = invisible. Look right at the top edge, behind the nav text.',
        min: 0,
        max: 0.3,
        step: 0.005,
        default: 0,
        unit: '',
      },
      {
        prop: '--crt-glow-peak',
        label: 'Flicker peak',
        hint: 'Brightness at the top of its pulse. Set higher than "Resting" or the pulse is invisible.',
        min: 0,
        max: 0.4,
        step: 0.005,
        default: 0.4,
        unit: '',
      },
      {
        prop: '--crt-glow-flicker-duration',
        label: 'Flicker period (s)',
        hint: 'How long one full resting-to-peak-and-back pulse takes.',
        min: 0.5,
        max: 15,
        step: 0.1,
        default: 5.3,
        unit: 's',
      },
    ],
  },
  {
    id: 'halation',
    label: 'Halation',
    hint: 'Blurs + brightens a copy of the whole page, screened back on top — a soft overall bloom. No animation.',
    knobs: [
      {
        prop: '--crt-halation-opacity',
        label: 'Strength',
        hint: '0 = off. Too high washes out contrast everywhere, not just bright spots.',
        min: 0,
        max: 0.4,
        step: 0.005,
        default: 0.225,
        unit: '',
      },
      {
        prop: '--crt-halation-blur',
        label: 'Blur radius (px)',
        hint: 'How soft the bloom is. Larger = a wider, hazier glow; smaller = a tighter one that hugs edges more closely.',
        min: 0,
        max: 20,
        step: 0.5,
        default: 4,
        unit: 'px',
      },
    ],
  },
  {
    id: 'beam',
    label: 'Beam',
    hint: "A light band sweeping top-to-bottom over ~9s, confined to the hero's flow-field canvas only — not the nav bar, not the footer.",
    knobs: [
      {
        prop: '--crt-beam-opacity',
        label: 'Strength',
        hint: "0 = nothing moves. Watch the canvas art (not the text) for a full pass if you're not sure it's on.",
        min: 0,
        max: 1,
        step: 0.01,
        default: 0.4,
        unit: '',
      },
      {
        prop: '--crt-beam-duration',
        label: 'Sweep period (s)',
        hint: 'How long one top-to-bottom pass takes.',
        min: 1,
        max: 30,
        step: 0.5,
        default: 9,
        unit: 's',
      },
      {
        prop: '--crt-beam-height',
        label: 'Band height (vh)',
        hint: 'How tall the light band is relative to the viewport. Larger = a broader, softer sweep; smaller = a thinner, more defined one.',
        min: 5,
        max: 60,
        step: 1,
        default: 20,
        unit: 'vh',
      },
    ],
  },
  {
    id: 'noise',
    label: 'Grain',
    hint: 'Film-grain static over the whole page, jittering in 5 discrete steps roughly every 0.7s — not a smooth crossfade.',
    knobs: [
      {
        prop: '--crt-noise-opacity',
        label: 'Strength',
        hint: 'Subtle by design. Push it up to see the stepping clearly, then back off.',
        min: 0,
        max: 0.15,
        step: 0.002,
        default: 0.15,
        unit: '',
      },
      {
        prop: '--crt-noise-step-duration',
        label: 'Step period (s)',
        hint: 'How long each of the 5 grain steps holds before jumping to the next.',
        min: 0.1,
        max: 3,
        step: 0.05,
        default: 1.2,
        unit: 's',
      },
    ],
  },
  {
    id: 'aberration',
    label: 'Chromatic aberration',
    hint: 'Red/cyan colour-fringing. On text it scales with font-size; on the flow-field canvas and the GitHub icon it applies at full strength always (text-shadow has no effect on those, so they get a separate always-on treatment).',
    knobs: [
      {
        prop: '--crt-ab-target',
        label: 'Max (device px)',
        hint: 'Ceiling for both: the headline and non-text elements reach this; smaller text stays well under it.',
        min: 0,
        max: 6,
        step: 0.1,
        default: 0.5,
        unit: 'px',
      },
      {
        prop: '--crt-ab-ref-size',
        label: 'Ramp reference size',
        hint: 'Font-size at which text reaches the full Max above. Lower = more body text fringes; raise = only large headlines do.',
        min: 12,
        max: 80,
        step: 1,
        default: 30,
        unit: 'px',
      },
    ],
  },
]

const KNOBS: Knob[] = LAYERS.flatMap((layer) => layer.knobs)

const STORAGE_KEY = 'tauler:crt-tuning'
const HIDDEN_KEY = 'tauler:crt-hidden'
const WARP_KEY = 'tauler:crt-page-warp'
const WARP_STRENGTH_KEY = 'tauler:crt-page-warp-strength'
const WARP_STRENGTH_DEFAULT = 40
const BOW_ENABLED_KEY = 'tauler:crt-bow-enabled'
const TEXT_ATTEN_KEY = 'tauler:crt-text-attenuation'
const TEXT_ATTEN_DEFAULT = 0
const NOISE_VARIANT_KEY = 'tauler:crt-noise-variant'
const FLICKER_VARIANT_KEY = 'tauler:crt-flicker-variant'
const FRAME_KEY = 'tauler:crt-frame'

function readStored(): Record<string, number> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}')
  } catch {
    return {}
  }
}

function writeStored(values: Record<string, number>): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(values))
}

function readHidden(): Set<string> {
  try {
    const list = JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? '[]')
    return new Set(Array.isArray(list) ? list : [])
  } catch {
    return new Set()
  }
}

function applyHidden(hidden: Set<string>): void {
  document.documentElement.setAttribute(
    'data-crt-hide',
    Array.from(hidden).join(' '),
  )
  localStorage.setItem(HIDDEN_KEY, JSON.stringify(Array.from(hidden)))
}

function applyKnob(knob: Knob, value: number): void {
  document.documentElement.style.setProperty(
    knob.prop,
    `${value}${knob.unit}`,
  )
  // --crt-ab-target also drives the canvas/SVG fringe filter, which
  // can't read the CSS custom property itself — feOffset's dx is an SVG
  // geometry attribute, not something url(#filter) makes reactive to
  // the referencing element's custom properties the way text-shadow's
  // calc() is. Kept in sync here rather than duplicated at every call
  // site that changes this one knob.
  if (knob.prop === '--crt-ab-target') {
    const dpr = Number(
      getComputedStyle(document.documentElement).getPropertyValue(
        '--crt-dpr',
      ) || 1,
    )
    setCanvasFringe(value / (dpr || 1))
  }
}

function applyVariantAttr(attr: string, key: string, onValue: string): void {
  if (localStorage.getItem(key) === '1') {
    document.documentElement.setAttribute(attr, onValue)
  }
}

// A checkbox that toggles an html[attr="onValue"] selector on/off,
// persisted as '0'/'1' under storageKey. Used for the three variant
// pairs (static grain, classic flicker, frame bezel) — each is a plain
// attribute swap in crt.css, no CSS custom property involved, so this
// doesn't reuse the KNOBS slider machinery above.
function buildVariantToggle(opts: {
  label: string
  hint: string
  attr: string
  onValue: string
  storageKey: string
}): HTMLElement {
  const wrap = document.createElement('div')
  Object.assign(wrap.style, {
    marginTop: '8px',
  } satisfies Partial<CSSStyleDeclaration>)

  const row = document.createElement('label')
  Object.assign(row.style, {
    display: 'flex',
    alignItems: 'center',
    gap: '6px',
    cursor: 'pointer',
  } satisfies Partial<CSSStyleDeclaration>)

  const checkbox = document.createElement('input')
  checkbox.type = 'checkbox'
  checkbox.checked = localStorage.getItem(opts.storageKey) === '1'
  checkbox.addEventListener('change', () => {
    localStorage.setItem(opts.storageKey, checkbox.checked ? '1' : '0')
    if (checkbox.checked) {
      document.documentElement.setAttribute(opts.attr, opts.onValue)
    } else {
      document.documentElement.removeAttribute(opts.attr)
    }
  })

  const label = document.createElement('span')
  label.textContent = opts.label
  row.append(checkbox, label)

  const hint = document.createElement('div')
  hint.textContent = opts.hint
  Object.assign(hint.style, {
    fontSize: '10px',
    lineHeight: '1.35',
    opacity: '0.55',
    marginTop: '2px',
    marginLeft: '20px',
  } satisfies Partial<CSSStyleDeclaration>)

  wrap.append(row, hint)
  return wrap
}

// Reapply any saved tuning on every load, panel open or not — the whole
// point is that adjustments survive a refresh while you're dialing
// something in. Values are clamped to the CURRENT knob range: a value
// saved under an earlier, wider slider range (or from an earlier
// version of the effect this knob drove) would otherwise get reapplied
// unclamped and silently out of range — invisible in the panel, which
// only ever shows the current min/max, but still live on the page.
function applyStored(): void {
  const stored = readStored()
  for (const knob of KNOBS) {
    if (knob.prop in stored) {
      const clamped = Math.min(
        knob.max,
        Math.max(knob.min, stored[knob.prop]),
      )
      applyKnob(knob, clamped)
    }
  }
  // Unconditional, unlike the loop above: on a totally fresh load with
  // nothing in storage, --crt-ab-target still resolves to crt.css's own
  // shipped default (the stylesheet sets it directly, not just as a
  // var() fallback) — but the canvas/SVG fringe filter has no CSS-level
  // default of its own, only whatever setCanvasFringe was last called
  // with. Without this, a fresh page load would show text fringing but
  // zero fringing on the canvas and the GitHub icon.
  {
    const dpr = Number(
      getComputedStyle(document.documentElement).getPropertyValue(
        '--crt-dpr',
      ) || 1,
    )
    const abTarget = parseFloat(
      getComputedStyle(document.documentElement).getPropertyValue(
        '--crt-ab-target',
      ),
    )
    if (!Number.isNaN(abTarget)) {
      setCanvasFringe(abTarget / (dpr || 1))
    }
  }
  if (localStorage.getItem(BOW_ENABLED_KEY) === '0') {
    document.documentElement.setAttribute('data-crt-bow-off', '')
  }
  applyVariantAttr('data-crt-noise-variant', NOISE_VARIANT_KEY, 'static')
  applyVariantAttr(
    'data-crt-scanline-flicker-variant',
    FLICKER_VARIANT_KEY,
    'classic',
  )
  applyVariantAttr('data-crt-frame', FRAME_KEY, 'on')
  const storedAtten = Number(
    localStorage.getItem(TEXT_ATTEN_KEY) ?? TEXT_ATTEN_DEFAULT,
  )
  if (storedAtten > 0) setTextAttenuation(storedAtten)
  applyHidden(readHidden())
  if (localStorage.getItem(WARP_KEY) === '1') {
    setPageWarp(true)
    const storedStrength = localStorage.getItem(WARP_STRENGTH_KEY)
    if (storedStrength !== null) {
      setPageWarpStrength(Number(storedStrength))
    }
  }
}

let panelEl: HTMLElement | null = null

function buildPanel(): HTMLElement {
  const stored = readStored()
  const hidden = readHidden()

  const panel = document.createElement('div')
  panel.setAttribute('aria-label', 'CRT tuning panel')
  Object.assign(panel.style, {
    position: 'fixed',
    top: '12px',
    right: '12px',
    zIndex: '2147483647',
    width: '320px',
    maxHeight: 'calc(100vh - 24px)',
    overflowY: 'auto',
    background: 'rgba(10, 10, 14, 0.94)',
    border: '1px solid rgba(255, 255, 255, 0.16)',
    borderRadius: '4px',
    padding: '10px 12px 14px',
    font: '11px/1.4 ui-monospace, monospace',
    color: '#e8e8ee',
    pointerEvents: 'auto',
  } satisfies Partial<CSSStyleDeclaration>)

  const title = document.createElement('div')
  title.textContent = 'crt.tune()'
  Object.assign(title.style, {
    fontWeight: '600',
    letterSpacing: '0.08em',
    textTransform: 'uppercase',
    marginBottom: '2px',
    opacity: '0.8',
  } satisfies Partial<CSSStyleDeclaration>)
  panel.appendChild(title)

  const subtitle = document.createElement('div')
  subtitle.textContent =
    'Uncheck a layer to isolate the others — with everything stacked at once, effects are easy to mix up.'
  Object.assign(subtitle.style, {
    fontSize: '10px',
    opacity: '0.5',
    marginBottom: '8px',
    lineHeight: '1.35',
  } satisfies Partial<CSSStyleDeclaration>)
  panel.appendChild(subtitle)

  for (const layer of LAYERS) {
    const section = document.createElement('div')
    Object.assign(section.style, {
      marginTop: '10px',
      paddingTop: '10px',
      borderTop: '1px solid rgba(255, 255, 255, 0.1)',
    } satisfies Partial<CSSStyleDeclaration>)

    const header = document.createElement('label')
    Object.assign(header.style, {
      display: 'flex',
      alignItems: 'center',
      gap: '6px',
      fontWeight: '600',
      cursor: 'pointer',
    } satisfies Partial<CSSStyleDeclaration>)

    const checkbox = document.createElement('input')
    checkbox.type = 'checkbox'
    checkbox.checked = !hidden.has(layer.id)
    checkbox.addEventListener('change', () => {
      const next = readHidden()
      if (checkbox.checked) next.delete(layer.id)
      else next.add(layer.id)
      applyHidden(next)
      knobsWrap.style.opacity = checkbox.checked ? '1' : '0.35'
    })

    const headerLabel = document.createElement('span')
    headerLabel.textContent = layer.label
    header.append(checkbox, headerLabel)
    section.appendChild(header)

    const layerHint = document.createElement('div')
    layerHint.textContent = layer.hint
    Object.assign(layerHint.style, {
      fontSize: '10px',
      lineHeight: '1.35',
      opacity: '0.5',
      margin: '3px 0 6px 20px',
    } satisfies Partial<CSSStyleDeclaration>)
    section.appendChild(layerHint)

    const knobsWrap = document.createElement('div')
    Object.assign(knobsWrap.style, {
      marginLeft: '20px',
      opacity: hidden.has(layer.id) ? '0.35' : '1',
    } satisfies Partial<CSSStyleDeclaration>)

    for (const knob of layer.knobs) {
      const raw = stored[knob.prop] ?? knob.default
      const current = Math.min(knob.max, Math.max(knob.min, raw))

      const row = document.createElement('label')
      Object.assign(row.style, {
        display: 'block',
        marginTop: '6px',
      } satisfies Partial<CSSStyleDeclaration>)

      const labelRow = document.createElement('div')
      Object.assign(labelRow.style, {
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        opacity: '0.85',
        marginBottom: '2px',
      } satisfies Partial<CSSStyleDeclaration>)
      const labelLeft = document.createElement('span')
      Object.assign(labelLeft.style, {
        display: 'flex',
        alignItems: 'center',
        gap: '5px',
      } satisfies Partial<CSSStyleDeclaration>)
      const labelText = document.createElement('span')
      labelText.textContent = knob.label
      labelLeft.appendChild(labelText)
      const valueText = document.createElement('span')
      valueText.textContent = String(current)
      labelRow.append(labelLeft, valueText)

      const input = document.createElement('input')
      input.type = 'range'
      input.min = String(knob.min)
      input.max = String(knob.max)
      input.step = String(knob.step)
      input.value = String(current)
      Object.assign(input.style, {
        width: '100%',
        accentColor: '#f0855a',
      } satisfies Partial<CSSStyleDeclaration>)

      input.addEventListener('input', () => {
        const value = Number(input.value)
        valueText.textContent = String(value)
        applyKnob(knob, value)
        const next = readStored()
        next[knob.prop] = value
        writeStored(next)
      })

      // The bow is the one knob users want to A/B against a genuinely
      // flat baseline, not just dial down. This used to fake "flat" by
      // pushing --crt-bow to an astronomical value — visually wrong: an
      // extreme Ry:Rx ratio on the ellipse gradient degenerates into
      // thick vertical bands (the same anisotropic-gradient problem
      // that caused the diagonal bug, inverted). A real flat fallback
      // needs to skip the ellipse math entirely — html[data-crt-bow-off]
      // swaps to a plain repeating-linear-gradient instead (see
      // crt.css). This toggle sets that attribute rather than touching
      // --crt-bow at all, so the slider's own value is untouched by it.
      if (knob.prop === '--crt-bow') {
        const bowToggle = document.createElement('input')
        bowToggle.type = 'checkbox'
        bowToggle.checked = localStorage.getItem(BOW_ENABLED_KEY) !== '0'
        bowToggle.title = 'Bow on/off'
        bowToggle.addEventListener('change', () => {
          localStorage.setItem(
            BOW_ENABLED_KEY,
            bowToggle.checked ? '1' : '0',
          )
          input.disabled = !bowToggle.checked
          document.documentElement.toggleAttribute(
            'data-crt-bow-off',
            !bowToggle.checked,
          )
        })
        labelLeft.prepend(bowToggle)
        input.disabled = !bowToggle.checked
        document.documentElement.toggleAttribute(
          'data-crt-bow-off',
          !bowToggle.checked,
        )
      }

      const hint = document.createElement('div')
      hint.textContent = knob.hint
      Object.assign(hint.style, {
        fontSize: '10px',
        lineHeight: '1.35',
        opacity: '0.55',
        marginTop: '2px',
      } satisfies Partial<CSSStyleDeclaration>)

      row.append(labelRow, input, hint)
      knobsWrap.appendChild(row)
    }

    // Text attenuation isn't a KNOBS entry: it doesn't set a CSS custom
    // property, it rebuilds a canvas-rasterised mask (see
    // crt-text-mask.ts) covering every text-bearing element's bounding
    // box, so it needs its own JS call rather than a var() the
    // stylesheet already knows how to read.
    if (layer.id === 'scanlines') {
      const storedAtten = Number(
        localStorage.getItem(TEXT_ATTEN_KEY) ?? TEXT_ATTEN_DEFAULT,
      )

      const attenRow = document.createElement('label')
      Object.assign(attenRow.style, {
        display: 'block',
        marginTop: '6px',
      } satisfies Partial<CSSStyleDeclaration>)

      const attenLabelRow = document.createElement('div')
      Object.assign(attenLabelRow.style, {
        display: 'flex',
        justifyContent: 'space-between',
        opacity: '0.85',
        marginBottom: '2px',
      } satisfies Partial<CSSStyleDeclaration>)
      const attenLabelText = document.createElement('span')
      attenLabelText.textContent = 'Text attenuation'
      const attenValueText = document.createElement('span')
      attenValueText.textContent = String(storedAtten)
      attenLabelRow.append(attenLabelText, attenValueText)

      const attenInput = document.createElement('input')
      attenInput.type = 'range'
      attenInput.min = '0'
      attenInput.max = '1'
      attenInput.step = '0.05'
      attenInput.value = String(storedAtten)
      Object.assign(attenInput.style, {
        width: '100%',
        accentColor: '#f0855a',
      } satisfies Partial<CSSStyleDeclaration>)
      attenInput.addEventListener('input', () => {
        const value = Number(attenInput.value)
        attenValueText.textContent = String(value)
        localStorage.setItem(TEXT_ATTEN_KEY, String(value))
        setTextAttenuation(value)
      })

      const attenHint = document.createElement('div')
      attenHint.textContent =
        "Reduces the scanline mesh's darkening specifically over text (headline, body copy, buttons, nav) — the rest of the mesh is unaffected. 0 = no difference from ordinary text. 1 = scanlines fully cleared wherever there's text."
      Object.assign(attenHint.style, {
        fontSize: '10px',
        lineHeight: '1.35',
        opacity: '0.55',
        marginTop: '2px',
      } satisfies Partial<CSSStyleDeclaration>)

      attenRow.append(attenLabelRow, attenInput, attenHint)
      knobsWrap.appendChild(attenRow)

      if (storedAtten > 0) setTextAttenuation(storedAtten)

      knobsWrap.appendChild(
        buildVariantToggle({
          label: 'Classic flicker variant',
          hint: "Lucas Bebber's original canon technique (codepen.io/lbebber/pen/XJRdrV): 21 fixed keyframe stops of pseudo-random opacity over 0.15s (~133fps), instead of the smooth breathing pulse above. This is the exact pattern crt-resources.md flags as a real photosensitivity/WCAG flash risk, not just a style choice — still fully killed by prefers-reduced-motion, but treat this as a comparison, not a default candidate.",
          attr: 'data-crt-scanline-flicker-variant',
          onValue: 'classic',
          storageKey: FLICKER_VARIANT_KEY,
        }),
      )

      knobsWrap.appendChild(
        buildVariantToggle({
          label: 'Frame bezel variant (experimental)',
          hint: "The other canon approach to a curved screen (codepen.io/somethingformed/pen/raWJXV): keep the scanline mesh flat and suggest curvature with a physical bezel around the hero instead — rounded corners, shaded borders, heavy inset shadow. This directly contradicts --radius: 0 elsewhere in the design (global.css calls sharp corners \"the whole visual argument\"), so it's here purely for comparison, independent of the Bow toggle above.",
          attr: 'data-crt-frame',
          onValue: 'on',
          storageKey: FRAME_KEY,
        }),
      )
    }

    if (layer.id === 'noise') {
      knobsWrap.appendChild(
        buildVariantToggle({
          label: 'Static texture variant',
          hint: 'Swaps the animated SVG-turbulence grain for a fixed noise texture (codepen.io/somethingformed/pen/raWJXV) — no jitter at all, cheaper, and a genuinely different look rather than a tuned-down version of the same one.',
          attr: 'data-crt-noise-variant',
          onValue: 'static',
          storageKey: NOISE_VARIANT_KEY,
        }),
      )
    }

    section.appendChild(knobsWrap)
    panel.appendChild(section)
  }

  // Experimental page-warp toggle — not a LAYERS entry, because turning
  // it on/off calls into crt-warp.ts (builds a canvas-generated
  // displacement map and sets a real filter on <main>) rather than just
  // flipping a CSS custom property. Off by default: this actually
  // displaces the real hero content, not just a decorative overlay.
  const warpSection = document.createElement('div')
  Object.assign(warpSection.style, {
    marginTop: '10px',
    paddingTop: '10px',
    borderTop: '1px solid rgba(255, 255, 255, 0.1)',
  } satisfies Partial<CSSStyleDeclaration>)

  const warpHeader = document.createElement('label')
  Object.assign(warpHeader.style, {
    display: 'flex',
    alignItems: 'center',
    gap: '6px',
    fontWeight: '600',
    cursor: 'pointer',
    color: '#f0855a',
  } satisfies Partial<CSSStyleDeclaration>)

  const warpCheckbox = document.createElement('input')
  warpCheckbox.type = 'checkbox'
  warpCheckbox.checked = localStorage.getItem(WARP_KEY) === '1'
  warpCheckbox.addEventListener('change', () => {
    localStorage.setItem(WARP_KEY, warpCheckbox.checked ? '1' : '0')
    setPageWarp(warpCheckbox.checked)
  })

  const warpLabel = document.createElement('span')
  warpLabel.textContent = 'EXPERIMENTAL: bend real content'
  warpHeader.append(warpCheckbox, warpLabel)
  warpSection.appendChild(warpHeader)

  const warpHint = document.createElement('div')
  warpHint.textContent =
    "Bends the actual hero — not just the scanline overlay — using an SVG displacement filter on <main>. Displacement is ~0 over the real content column (headline, buttons, command line) and only grows in the empty background to the right, so click targets stay where they visually appear. Disabled below 900px viewport width, where there's no empty background to bend into."
  Object.assign(warpHint.style, {
    fontSize: '10px',
    lineHeight: '1.35',
    opacity: '0.55',
    margin: '3px 0 0 20px',
  } satisfies Partial<CSSStyleDeclaration>)
  warpSection.appendChild(warpHint)

  const warpStrengthWrap = document.createElement('div')
  Object.assign(warpStrengthWrap.style, {
    marginLeft: '20px',
    marginTop: '6px',
    opacity: warpCheckbox.checked ? '1' : '0.35',
  } satisfies Partial<CSSStyleDeclaration>)

  const storedStrength = Number(
    localStorage.getItem(WARP_STRENGTH_KEY) ?? WARP_STRENGTH_DEFAULT,
  )

  const strengthLabelRow = document.createElement('div')
  Object.assign(strengthLabelRow.style, {
    display: 'flex',
    justifyContent: 'space-between',
    opacity: '0.85',
    marginBottom: '2px',
  } satisfies Partial<CSSStyleDeclaration>)
  const strengthLabelText = document.createElement('span')
  strengthLabelText.textContent = 'Strength'
  const strengthValueText = document.createElement('span')
  strengthValueText.textContent = String(storedStrength)
  strengthLabelRow.append(strengthLabelText, strengthValueText)

  const strengthInput = document.createElement('input')
  strengthInput.type = 'range'
  strengthInput.min = '0'
  strengthInput.max = '150'
  strengthInput.step = '2'
  strengthInput.value = String(storedStrength)
  Object.assign(strengthInput.style, {
    width: '100%',
    accentColor: '#f0855a',
  } satisfies Partial<CSSStyleDeclaration>)
  strengthInput.addEventListener('input', () => {
    const value = Number(strengthInput.value)
    strengthValueText.textContent = String(value)
    localStorage.setItem(WARP_STRENGTH_KEY, String(value))
    setPageWarpStrength(value)
  })

  const strengthHint = document.createElement('div')
  strengthHint.textContent =
    'How far the background bends at its most-displaced point (empty space, far from the content column). 0 = flat. The default (40) is a moderate bend — push well past 100 to see how far this can go before it looks broken.'
  Object.assign(strengthHint.style, {
    fontSize: '10px',
    lineHeight: '1.35',
    opacity: '0.55',
    marginTop: '2px',
  } satisfies Partial<CSSStyleDeclaration>)

  warpStrengthWrap.append(strengthLabelRow, strengthInput, strengthHint)
  warpSection.appendChild(warpStrengthWrap)

  warpCheckbox.addEventListener('change', () => {
    warpStrengthWrap.style.opacity = warpCheckbox.checked ? '1' : '0.35'
  })

  // Apply the stored strength immediately if the effect starts checked
  // (panel reopened after being left on) — setPageWarp(true) above
  // rebuilds the filter at the hardcoded default, so this corrects it
  // to whatever was last saved.
  if (warpCheckbox.checked) setPageWarpStrength(storedStrength)

  panel.appendChild(warpSection)

  const actions = document.createElement('div')
  Object.assign(actions.style, {
    display: 'flex',
    gap: '6px',
    marginTop: '12px',
  } satisfies Partial<CSSStyleDeclaration>)

  const makeButton = (
    text: string,
    onClick: () => void,
  ): HTMLButtonElement => {
    const btn = document.createElement('button')
    btn.type = 'button'
    btn.textContent = text
    Object.assign(btn.style, {
      flex: '1',
      font: 'inherit',
      padding: '5px 6px',
      background: 'rgba(255, 255, 255, 0.08)',
      color: 'inherit',
      border: '1px solid rgba(255, 255, 255, 0.2)',
      borderRadius: '3px',
      cursor: 'pointer',
    } satisfies Partial<CSSStyleDeclaration>)
    btn.addEventListener('click', onClick)
    return btn
  }

  actions.append(
    makeButton('Copy CSS', () => {
      const lines = KNOBS.map((knob) => {
        const current = readStored()[knob.prop] ?? knob.default
        return `  ${knob.prop}: ${current}${knob.unit};`
      })
      const css = `:root {\n${lines.join('\n')}\n}`
      console.log(css)
      navigator.clipboard?.writeText(css).catch(() => {})
    }),
    makeButton('Show all / Reset', () => {
      localStorage.removeItem(STORAGE_KEY)
      localStorage.removeItem(HIDDEN_KEY)
      localStorage.removeItem(WARP_KEY)
      localStorage.removeItem(WARP_STRENGTH_KEY)
      localStorage.removeItem(BOW_ENABLED_KEY)
      localStorage.removeItem(NOISE_VARIANT_KEY)
      localStorage.removeItem(FLICKER_VARIANT_KEY)
      localStorage.removeItem(FRAME_KEY)
      document.documentElement.removeAttribute('data-crt-noise-variant')
      document.documentElement.removeAttribute('data-crt-scanline-flicker-variant')
      document.documentElement.removeAttribute('data-crt-frame')
      document.documentElement.removeAttribute('data-crt-bow-off')
      localStorage.removeItem(TEXT_ATTEN_KEY)
      setTextAttenuation(0)
      for (const knob of KNOBS) {
        document.documentElement.style.removeProperty(knob.prop)
      }
      document.documentElement.removeAttribute('data-crt-hide')
      setPageWarp(false)
      hide()
      show()
    }),
    makeButton('Close', () => hide()),
  )
  panel.appendChild(actions)

  return panel
}

function show(): void {
  if (panelEl) return
  panelEl = buildPanel()
  document.body.appendChild(panelEl)
}

function hide(): void {
  panelEl?.remove()
  panelEl = null
}

export function initCrtPanel(): void {
  applyStored()
  // Tuning phase: show by default instead of requiring crt.tune() in the
  // console, so it's there on every reload while dialing in defaults.
  // Revert this once a final config is settled — a hidden panel that's
  // always open by default is a bug, not a feature, for anyone who
  // isn't actively tuning it.
  show()
  ;(window as unknown as { crt: Record<string, () => void> }).crt = {
    tune: show,
    hide,
    reset: () => {
      localStorage.removeItem(STORAGE_KEY)
      localStorage.removeItem(HIDDEN_KEY)
      localStorage.removeItem(WARP_KEY)
      localStorage.removeItem(WARP_STRENGTH_KEY)
      localStorage.removeItem(BOW_ENABLED_KEY)
      localStorage.removeItem(NOISE_VARIANT_KEY)
      localStorage.removeItem(FLICKER_VARIANT_KEY)
      localStorage.removeItem(FRAME_KEY)
      document.documentElement.removeAttribute('data-crt-noise-variant')
      document.documentElement.removeAttribute('data-crt-scanline-flicker-variant')
      document.documentElement.removeAttribute('data-crt-frame')
      document.documentElement.removeAttribute('data-crt-bow-off')
      localStorage.removeItem(TEXT_ATTEN_KEY)
      setTextAttenuation(0)
      for (const knob of KNOBS) {
        document.documentElement.style.removeProperty(knob.prop)
      }
      document.documentElement.removeAttribute('data-crt-hide')
      setPageWarp(false)
      hide()
    },
  }
}
