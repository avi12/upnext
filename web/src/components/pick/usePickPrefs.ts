import { useState } from 'react'
import type { Source } from '../../types'
import { SOURCES, loadJson, saveJson } from '../../lib/format'

const SOURCES_KEY = 'upnext.pick.sources'
const COUNT_KEY = 'upnext.pick.count'

/** How many picks, from which stores: remembered in this browser. */
export function usePickPrefs() {
  const [count, setCountState] = useState(() => {
    const n = loadJson<number>(COUNT_KEY, 3)
    return Number.isInteger(n) && n >= 1 && n <= 5 ? n : 3
  })
  const setCount = (n: number) => {
    setCountState(n)
    saveJson(COUNT_KEY, n)
  }
  const [sources, setSources] = useState<Source[]>(() => {
    const saved = loadJson<string[]>(SOURCES_KEY, SOURCES)
    const valid = SOURCES.filter((s) => saved.includes(s))
    return valid.length ? valid : SOURCES
  })
  const toggleSource = (s: Source) => {
    const next = sources.includes(s) ? sources.filter((x) => x !== s) : SOURCES.filter((x) => x === s || sources.includes(x))
    if (!next.length) return
    setSources(next)
    saveJson(SOURCES_KEY, next)
  }
  return { count, setCount, sources, toggleSource }
}
