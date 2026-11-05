'use client'
import { useState } from 'react'
import { Button } from '@/components/ui/button'

/** The install command is a placeholder until there is a real one. */
export function Terminal() {
  const command = '[INSTALL COMMAND]'
  const [copied, setCopied] = useState(false)
  return (
    <div className="win border-(--tu-rule)">
      <div className="wbar">terminal</div>
      <div className="m flex flex-wrap items-center gap-3 bg-(--tu-code) py-3.5 pr-3.5 pl-[18px] text-(--tu-codeink)">
        <code className="flex-[1_1_220px] text-[15px]">
          <span className="text-(--tu-or)">$</span> {command}
        </code>
        <Button
          variant="outline-primary"
          className="px-4"
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
