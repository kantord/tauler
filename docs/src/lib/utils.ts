// The `cn` that shadcn/ui's init writes: shadcn's registry components import
// it, and Fumadocs CLI components import it from here (components.json's
// `utils` alias), so both share one class merger.
//
// The merger has to know the design system's own utilities (global.css:
// the type scale and border-heavy / border-thin). Unregistered, it reads
// `text-label` as a colour and `border-heavy` as a border colour, and drops
// them next to `text-foreground` or `border-input`.
import { createCn } from 'cn/config'

export const cn = createCn({
  extend: {
    classGroups: {
      'font-size': [
        {
          text: [
            'caption',
            'label',
            'label-strong',
            'display-lg',
            'display-xl',
          ],
        },
      ],
      'border-w': [{ border: ['heavy', 'thin'] }],
      'border-w-t': [{ 'border-t': ['heavy'] }],
      'border-w-b': [{ 'border-b': ['heavy'] }],
      'border-w-l': [{ 'border-l': ['heavy'] }],
    },
  },
})
