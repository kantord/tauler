// Page template lint (docs/AUTHORING.md). Every page under content/docs needs a
// `description` with no colon in it: a colon there once broke the front-matter
// parse. A page that names a template in its front matter (`template: article`
// or `template: service`) must also have that template's headings, in order.
//
//   node scripts/check-pages.mjs                 scan content/docs
//   node scripts/check-pages.mjs --root <dir>    scan another tree
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const ARTICLE = [
  'How it works',
  'Common tasks',
  'Rules',
  'Reference',
  'Related',
]
const SERVICE_FIRST = ['Install', 'State it prints', 'Events it accepts']
// Folders where every page except index must say which template it follows.
const TEMPLATED = ['data/', 'interaction/']
// Words the dynamic-data vocabulary replaced. Checked in prose only.
const BANNED = /\b(intents?|channels?|actors?|messages?|commands?|modules?)\b/i

const args = process.argv.slice(2)
const rootArg = args.indexOf('--root')
const root =
  rootArg >= 0
    ? args[rootArg + 1]
    : fileURLToPath(new URL('../content/docs/', import.meta.url))

function* pages(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) yield* pages(path)
    else if (/\.mdx?$/.test(name)) yield path
  }
}

const sameList = (a, b) =>
  a.length === b.length && a.every((x, i) => x === b[i])

function check(path) {
  const rel = relative(root, path)
  const text = readFileSync(path, 'utf8')
  const errors = []
  const fm = text.match(/^---\n([\s\S]*?)\n---\n/)
  if (!fm) return [`${rel}: no front matter`]
  const field = (key) =>
    fm[1].match(new RegExp(`^${key}:[ \\t]*(.*)$`, 'm'))?.[1].trim()

  const description = field('description')
  if (!description) errors.push('no description')
  else if (description.includes(':'))
    errors.push('description contains a colon; reword it')

  const template = field('template')
  const mustDeclare =
    TEMPLATED.some((dir) => rel.startsWith(dir)) &&
    !/(^|\/)index\.mdx?$/.test(rel)
  if (!template) {
    if (mustDeclare)
      errors.push(
        'pages in this folder need `template: article` or `template: service`',
      )
    return errors.map((e) => `${rel}: ${e}`)
  }
  if (template !== 'article' && template !== 'service') {
    return [`${rel}: unknown template "${template}"`]
  }

  // Headings and prose, with fenced code left out.
  const body = text.slice(fm[0].length)
  const prose = body.replace(/^```[\s\S]*?^```[^\n]*$/gm, '')
  const h2 = [...prose.matchAll(/^## (.+)$/gm)].map((m) => m[1].trim())
  const intro = body.split(/^## /m)[0]

  if (!/^```/m.test(intro)) errors.push('no example before the first heading')
  if (field('title')?.includes('`'))
    errors.push('title names code; state the need instead')

  if (template === 'article') {
    if (!sameList(h2, ARTICLE))
      errors.push(
        `headings must be exactly: ${ARTICLE.join(' / ')} (found: ${h2.join(' / ') || 'none'})`,
      )
    if (!/\*\*[^*]+\*\*/.test(intro.replace(/^```[\s\S]*?^```[^\n]*$/gm, '')))
      errors.push('the opening must name the concept once, in bold')
    const section = (name) =>
      prose.split(new RegExp(`^## ${name}$`, 'm'))[1]?.split(/^## /m)[0] ?? ''
    if (!/^### /m.test(section('Common tasks')))
      errors.push('Common tasks needs at least one ### task')
    if (!/^### Behaviour$/m.test(section('Reference')))
      errors.push('Reference needs a ### Behaviour part')
  } else {
    if (!sameList(h2.slice(0, SERVICE_FIRST.length), SERVICE_FIRST))
      errors.push(`headings must start with: ${SERVICE_FIRST.join(' / ')}`)
    if (h2.at(-1) !== 'Related') errors.push('the last heading must be Related')
  }

  const words = prose.replace(/`[^`\n]*`/g, '').replace(/\]\([^)]*\)/g, ']')
  const banned = words.match(BANNED)
  if (banned)
    errors.push(
      `"${banned[0]}" is not in the vocabulary; see docs/AUTHORING.md`,
    )

  return errors.map((e) => `${rel}: ${e}`)
}

const errors = [...pages(root)].flatMap(check)
if (errors.length) {
  for (const e of errors) console.error(e)
  process.exit(1)
}
console.log('page templates ok')
