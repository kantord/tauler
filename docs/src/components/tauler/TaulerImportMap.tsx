import { readFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * The import map that makes a layout file's `import … from "@ui/card"`
 * resolve in a page. tauler-docgen writes it to .tauler/importmap.json with
 * the rest of the browser assets; without them (a checkout that never ran
 * `just docs`) there is no map, and the live examples fall back to their
 * screenshots anyway.
 *
 * It goes in the <head> of every docs page, not just the ones with examples:
 * an import map only counts if it is in the document before the first module
 * loads, and a client-side navigation never re-renders the <head>.
 */
export function TaulerImportMap() {
  let map: string
  try {
    map = readFileSync(join(process.cwd(), '.tauler', 'importmap.json'), 'utf8')
  } catch {
    return null
  }
  return <script type="importmap" dangerouslySetInnerHTML={{ __html: map }} />
}
