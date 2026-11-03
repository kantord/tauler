// The `cn` that shadcn/ui's init writes: shadcn's registry components import
// the `cn` package directly, and Fumadocs CLI components import it from here
// (components.json's `utils` alias), so both share one class merger.
export { cn } from 'cn'
