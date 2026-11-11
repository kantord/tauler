'use client'
import { useState } from 'react'
import { Button } from '@/components/ui/button'

/** The install command is a placeholder until there is a real one. */
export function Terminal() {
  const command = '[INSTALL COMMAND]'
  const [copied, setCopied] = useState(false)
  return (
    <div className="win border-heavy border-rule">
      <div className="wbar">terminal</div>
      <div className="m flex flex-wrap items-center gap-3 bg-code p-4 text-code-ink">
        <code className="flex-[1_1_220px] text-sm">
          <span className="text-code-orange">$</span> {command}
        </code>
        <Button
          variant="outline-primary"
          className="px-4 text-code-orange"
          onClick={() => {
            void navigator.clipboard?.writeText(command).then(() => {
              setCopied(true)
            })
          }}
        >
          {copied ? 'copied' : 'copy'}
        </Button>
      </div>
    </div>
  )
}
