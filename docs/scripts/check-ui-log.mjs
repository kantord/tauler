// Fails when a component in src/components/ui is not mentioned in EDITS.md.
// icon.tsx is the site's own component and is listed there as exempt.
import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const dir = fileURLToPath(new URL('../src/components/ui/', import.meta.url))
const log = readFileSync(`${dir}EDITS.md`, 'utf8')
const missing = readdirSync(dir).filter(
  (f) => f.endsWith('.tsx') && !log.includes(f),
)
if (missing.length) {
  console.error(
    `src/components/ui/EDITS.md does not mention: ${missing.join(', ')}`,
  )
  process.exit(1)
}
console.log('ui edit log ok')
