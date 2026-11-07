import type { ThemeRegistration } from 'shiki'

/**
 * The design system's code palette: the --code-* variables in global.css.
 * Code panels are dark in both site themes, so one theme serves both.
 */
export const taulerCodeTheme: ThemeRegistration = {
  name: 'tauler',
  type: 'dark',
  colors: {
    'editor.background': 'var(--code)',
    'editor.foreground': 'var(--code-ink)',
  },
  tokenColors: [
    {
      scope: ['comment', 'punctuation.definition.comment'],
      settings: { foreground: 'var(--code-muted)', fontStyle: 'italic' },
    },
    {
      scope: [
        'keyword',
        'storage',
        'storage.type',
        'storage.modifier',
        'keyword.control',
        'keyword.operator.new',
        'variable.language',
        'punctuation.definition.variable.shell',
      ],
      settings: { foreground: 'var(--code-orange)', fontStyle: 'bold' },
    },
    {
      scope: [
        'entity.name.tag',
        'support.class.component',
        'entity.name.function',
        'support.function',
        'entity.name.type',
      ],
      settings: { foreground: 'var(--code-ink)', fontStyle: 'bold' },
    },
    {
      scope: [
        'entity.other.attribute-name',
        'variable.parameter',
        'meta.object-literal.key',
      ],
      settings: { foreground: 'var(--code-soft)' },
    },
    {
      scope: ['string', 'string.quoted', 'string.template'],
      settings: { foreground: 'var(--code-yellow)' },
    },
    {
      scope: ['constant.numeric', 'constant.language', 'constant.character'],
      settings: { foreground: 'var(--code-orange-soft)' },
    },
    {
      scope: [
        'punctuation',
        'punctuation.definition.tag',
        'meta.brace',
        'punctuation.section.embedded',
      ],
      settings: { foreground: 'var(--code-muted)' },
    },
    {
      scope: ['keyword.operator'],
      settings: { foreground: 'var(--code-soft)' },
    },
  ],
}
