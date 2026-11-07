import type { ThemeRegistration } from 'shiki'

/**
 * The design's code palette. Code panels are dark in both site themes, so
 * one theme serves both. Colours are the design's `sx-*` tokens.
 */
export const taulerCodeTheme: ThemeRegistration = {
  name: 'tauler',
  type: 'dark',
  colors: {
    'editor.background': '#1c1c1b',
    'editor.foreground': '#f4f2ec',
  },
  tokenColors: [
    {
      scope: ['comment', 'punctuation.definition.comment'],
      settings: { foreground: '#8a877f', fontStyle: 'italic' },
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
      settings: { foreground: '#ff6a2e', fontStyle: 'bold' },
    },
    {
      scope: [
        'entity.name.tag',
        'support.class.component',
        'entity.name.function',
        'support.function',
        'entity.name.type',
      ],
      settings: { foreground: '#f4f2ec', fontStyle: 'bold' },
    },
    {
      scope: [
        'entity.other.attribute-name',
        'variable.parameter',
        'meta.object-literal.key',
      ],
      settings: { foreground: '#b9b6ad' },
    },
    {
      scope: ['string', 'string.quoted', 'string.template'],
      settings: { foreground: '#ffc21a' },
    },
    {
      scope: ['constant.numeric', 'constant.language', 'constant.character'],
      settings: { foreground: '#ff8f5e' },
    },
    {
      scope: [
        'punctuation',
        'punctuation.definition.tag',
        'meta.brace',
        'punctuation.section.embedded',
      ],
      settings: { foreground: '#8a877f' },
    },
    { scope: ['keyword.operator'], settings: { foreground: '#b9b6ad' } },
  ],
}
