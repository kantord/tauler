// Validates the static export (`pnpm run validate:html`) for invalid element
// nesting only — the class of bug a browser silently repairs and React then
// fails to hydrate, such as a <div> inside a <p>. The full recommended preset
// flags style choices in Next's and Fumadocs' markup that are not bugs.
export default {
  root: true,
  extends: [],
  rules: {
    'element-permitted-content': 'error',
    'element-permitted-parent': 'error',
    'element-permitted-order': 'error',
    'element-permitted-occurrences': 'error',
    'close-order': 'error',
  },
}
