import { useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, api } from '../../api'
import type { SeedApplied } from '../../types'
import { absoluteTime, monthYear, plural, relativeTime } from '../../lib/format'
import { appliedToast } from '../../lib/seed'
import { useToast } from '../useToast'
import { Row } from './parts'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** UpNext's shared game data: People say, Vibes and tags for popular
 *  games, applied without fetching reviews or asking the AI. Apply it from the
 *  data server, another address or a file; see what's applied; stop using it. */
export function SharedDataRow() {
  const qc = useQueryClient()
  const toast = useToast()
  const [open, setOpen] = useState(false)
  const [url, setUrl] = useState('')
  const fileInput = useRef<HTMLInputElement>(null)
  const status = useQuery({ queryKey: ['seed'], queryFn: api.seed, staleTime: 30_000 })
  const s = status.data

  const changed = () => {
    for (const key of ['seed', 'game', 'games', 'stats']) qc.invalidateQueries({ queryKey: [key] })
  }
  const applied = (r: SeedApplied) => {
    changed()
    setUrl('')
    toast.success(...appliedToast(r))
  }
  const apply = useMutation({ mutationFn: (to?: string) => api.applySeed(to), onSuccess: applied, onError: (e) => toast.error("Couldn't apply it", errText(e)) })
  const upload = useMutation({ mutationFn: api.uploadSeed, onSuccess: applied, onError: (e) => toast.error("Couldn't apply that file", errText(e)) })
  const remove = useMutation({
    mutationFn: api.removeSeed,
    onSuccess: (r) => {
      changed()
      toast.success('Stopped using shared data', `${plural(r.removed, 'summary', 'summaries')} removed; your AI writes its own on the next summaries run.`)
    },
    onError: (e) => toast.error("Couldn't remove it", errText(e)),
  })
  const busy = apply.isPending || upload.isPending || remove.isPending

  const summary = !s ? undefined : s.seeded > 0 ? (
    <span className="text-[13.5px] font-normal text-dim">
      · {s.seeded.toLocaleString()} {s.seeded === 1 ? 'game' : 'games'} from {monthYear(s.seed_date)}
    </span>
  ) : (
    <span className="text-[13.5px] font-normal text-dim">· not used</span>
  )

  return (
    <Row
      name={<>Shared game data {summary}</>}
      desc="Ready-made People say, Vibes and tags for popular games, free for anyone to use (CC0), so those games need no AI call and no review fetch."
      wrap
      right={
        <button type="button" className="btn-outline" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
          {open ? 'Close' : s && s.seeded > 0 ? 'Change' : 'Use it'}
        </button>
      }
    >
      {open && s && (
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <button type="button" className="btn-outline" disabled={busy} onClick={() => apply.mutate(undefined)}>
              {apply.isPending && !url ? 'Applying…' : 'Apply the newest'}
            </button>
            <button type="button" className="btn-outline" disabled={busy} onClick={() => fileInput.current?.click()}>
              {upload.isPending ? 'Applying…' : 'Apply a file…'}
            </button>
            <input
              ref={fileInput}
              type="file"
              accept=".gz,application/gzip"
              className="sr-only"
              tabIndex={-1}
              aria-hidden
              onChange={(e) => {
                const f = e.target.files?.[0]
                if (f) upload.mutate(f)
                e.target.value = ''
              }}
            />
            {s.seeded > 0 && (
              <button type="button" className="btn-quiet text-[13.5px] underline decoration-underline underline-offset-4" disabled={busy} onClick={() => remove.mutate()}>
                Stop using it
              </button>
            )}
          </div>
          <form
            className="flex flex-wrap items-end gap-x-3 gap-y-2"
            onSubmit={(e) => {
              e.preventDefault()
              if (url.trim()) apply.mutate(url.trim())
            }}
          >
            <label className="flex min-w-0 flex-1 basis-[260px] flex-col gap-1 text-[13.5px] text-dim">
              Or from an address
              <input className="field text-sm" type="url" value={url} placeholder={s.default_url} onChange={(e) => setUrl(e.target.value)} />
            </label>
            <button type="submit" className="btn-outline" disabled={busy || !url.trim()}>
              {apply.isPending && url ? 'Applying…' : 'Apply'}
            </button>
          </form>
          <p className="m-0 text-[13.5px] text-dim">
            Your own summaries stay when your AI wrote them with the same prompt or a newer one. Ones written with an older prompt are replaced, and
            stopping later leaves those games without a summary until your AI writes it again. Games you add later get theirs after a sync
            {s.kept ? '' : ', once data is applied'}.
          </p>
          {s.applies.length > 0 && (
            <ul className="m-0 flex list-none flex-col gap-1 p-0 font-mono text-[11px] text-dim" aria-label="Applied">
              {s.applies.map((a) => (
                <li key={`${a.applied_at}-${a.source}`} title={absoluteTime(a.applied_at)}>
                  {relativeTime(a.applied_at)}: {monthYear(a.seed_date)}, {a.written.toLocaleString()} written, {a.matched.toLocaleString()} matched,{' '}
                  {a.kept_own.toLocaleString()} of yours kept · {a.source}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </Row>
  )
}
