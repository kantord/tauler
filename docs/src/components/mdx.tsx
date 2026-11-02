import type { ComponentProps, ReactElement, ReactNode } from 'react'
import { isValidElement } from 'react'
import defaultMdxComponents from 'fumadocs-ui/mdx'
import type { MDXComponents } from 'mdx/types'
import { TaulerExample } from '@/components/tauler/TaulerExample'
import { TaulerExamples } from '@/components/tauler/TaulerExamples'

const SHELL = new Set(['sh', 'bash', 'shell', 'console', 'zsh'])

function languageOf(children: ReactNode): string | undefined {
  if (!isValidElement(children)) return undefined
  const cls = (children as ReactElement<{ className?: string }>).props.className
  return cls?.match(/language-(\S+)/)?.[1]
}

/** A code block as the design draws it: a titled sub-panel over dark code. */
function CodePanel({
  title,
  children,
  className: _className,
  style: _style,
  ...props
}: ComponentProps<'pre'> & { title?: string; icon?: unknown }) {
  delete (props as { icon?: unknown }).icon
  const lang = languageOf(children)
  const label =
    title ??
    (lang && SHELL.has(lang)
      ? 'terminal'
      : lang && lang !== 'plaintext'
        ? lang
        : 'code')
  return (
    <div className="sub">
      <div className="sbar">{label}</div>
      <pre className="code" {...props}>
        {children}
      </pre>
    </div>
  )
}

function CalloutContainer({
  type,
  children,
}: {
  type?: string
  children: ReactNode
}) {
  // A container without a title still gets the design's labelled bar.
  const hasTitle = Array.isArray(children)
    ? children.some((c) => isValidElement(c) && c.type === CalloutTitle)
    : isValidElement(children) && children.type === CalloutTitle
  const fallback = {
    info: 'note',
    warning: 'caution',
    error: 'danger',
    success: 'tip',
  }[type ?? 'info']
  return (
    <div className="sub note" role="note">
      {!hasTitle && <div className="sbar">{fallback ?? 'note'}</div>}
      {children}
    </div>
  )
}

function CalloutTitle({ children }: { children: ReactNode }) {
  return <div className="sbar">{children}</div>
}

function CalloutDescription({ children }: { children: ReactNode }) {
  return <div className="sb">{children}</div>
}

// Headings without Fumadocs' anchor-link decoration; ids still come from
// remark-heading, which is what the table of contents links to.
type HeadingProps = ComponentProps<'h2'>
const plain = (Tag: 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6') =>
  function Heading(props: HeadingProps) {
    return <Tag {...props} />
  }

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    h1: plain('h1'),
    h2: plain('h2'),
    h3: plain('h3'),
    h4: plain('h4'),
    h5: plain('h5'),
    h6: plain('h6'),
    pre: CodePanel,
    table: (props: ComponentProps<'table'>) => (
      // Focusable so a keyboard can scroll a table wider than the column.
      <div className="tbl" tabIndex={0}>
        <table {...props} />
      </div>
    ),
    CalloutContainer,
    CalloutTitle,
    CalloutDescription,
    TaulerExample,
    TaulerExamples,
    ...components,
  } satisfies MDXComponents
}

export const useMDXComponents = getMDXComponents

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>
}
