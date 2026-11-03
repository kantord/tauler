'use client'
import { useState } from 'react'

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
        <button
          type="button"
          className="m min-h-10 cursor-pointer border-[1.5px] border-(--tu-or) bg-transparent px-4 text-[12px] font-semibold tracking-[.08em] text-(--tu-or) uppercase"
          onClick={() => {
            void navigator.clipboard?.writeText(command).then(() => {
              setCopied(true)
            })
          }}
        >
          {copied ? 'copied' : 'copy'}
        </button>
      </div>
    </div>
  )
}
