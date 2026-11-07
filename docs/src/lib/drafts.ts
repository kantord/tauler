/**
 * Whether draft pages exist: true under `next dev`, false in the published
 * build. A draft page is a `page.draft.tsx` route file, which next.config.mjs
 * only treats as a page in development; anything that links to one checks
 * this first, so the export never links to a page it does not have.
 */
export const DRAFTS = process.env.NODE_ENV === 'development'
