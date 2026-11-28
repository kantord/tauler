'use client'
import { useEffect } from 'react'

/** Marks the page once content has scrolled beneath the header, so the
 * header can hang its hatch strip (global.css). */
export function ScrollEdge() {
  useEffect(() => {
    const update = () =>
      document.documentElement.toggleAttribute(
        'data-scrolled',
        window.scrollY > 8,
      )
    update()
    window.addEventListener('scroll', update, { passive: true })
    return () => {
      window.removeEventListener('scroll', update)
      document.documentElement.removeAttribute('data-scrolled')
    }
  }, [])
  return null
}
