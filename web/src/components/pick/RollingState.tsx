import { useEffect, useState } from 'react'
import { cx } from '../../lib/format'

const LINES = [
  'Reading your library…',
  'Weighing what you actually finish…',
  'Skipping the soundtracks and tools…',
  'Matching the mood to the tags…',
  'Checking how long each one takes…',
  'Arguing with itself about the last pick…',
  'Writing down why…',
]

/** Calm placeholder for the lineup while UpNext is curating it. */
export function RollingState({ startedAt, count }: { startedAt: number; count: number }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 500)
    return () => window.clearInterval(t)
  }, [])
  const elapsed = Math.max(0, Math.floor((now - startedAt) / 1000))
  const line = LINES[Math.floor(elapsed / 3) % LINES.length]
  const cols = Math.min(Math.max(count, 1), 3)

  return (
    <div role="status" aria-live="polite">
      <div className="flex items-baseline justify-between gap-4 border-b border-line pb-3.5">
        <div className="flex items-baseline gap-3.5">
          <span className="font-display text-xl font-bold">Curating</span>
          <span key={line} className="fade-up text-sm text-dim">
            {line}
          </span>
        </div>
        <span className="mono" aria-hidden="true">
          {elapsed}s · usually 20–90s
        </span>
      </div>
      <div className={cx('grid grid-cols-1 md:-mx-7', cols === 2 && 'md:grid-cols-2', cols === 3 && 'md:grid-cols-3')} aria-hidden="true">
        {Array.from({ length: cols }, (_, i) => (
          <div
            key={i}
            className={cx(
              'flex flex-col gap-3 pt-7 pb-8',
              'md:px-7',
              i > 0 && 'hidden md:flex md:border-l md:border-line',
            )}
          >
            <div className="h-10 w-7 rounded-md bg-raised" />
            <div className="shimmer aspect-[460/215] rounded-[10px]" />
            <div className="mt-1.5 h-6 w-2/3 rounded-md bg-raised" />
            <div className="h-3 w-1/2 rounded bg-raised" />
            <div className="mt-1 h-3.5 w-full rounded bg-raised" />
            <div className="h-3.5 w-11/12 rounded bg-raised" />
            <div className="h-3.5 w-4/5 rounded bg-raised" />
          </div>
        ))}
      </div>
    </div>
  )
}
