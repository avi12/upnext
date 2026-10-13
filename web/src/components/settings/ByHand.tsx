import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import { plural } from '../../lib/format'
import { keys } from '../../lib/queries'
import { useToast } from '../useToast'
import { Row, Segmented, Status, TextButton } from './parts'

/** Games added by hand, for the stores whose owned library UpNext can't
 *  read without a password: a list of titles per store, one a line,
 *  saved over the one before. `store` opens the row on that store's list. */
export function ByHandRow({ store: first }: { store?: string }) {
  const qc = useQueryClient()
  const toast = useToast()
  const lists = useQuery({ queryKey: ['byHand'], queryFn: api.byHand, staleTime: 60_000 })
  const [open, setOpen] = useState(false)
  const [store, setStore] = useState(first ?? 'origin')
  // The text being edited, by store; a store not edited shows its saved list.
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const stores = lists.data?.stores ?? []
  const current = stores.find((s) => s.store === store)
  const saved = current?.titles.join('\n') ?? ''
  const text = drafts[store] ?? saved
  const save = useMutation({
    mutationFn: () => api.byHandSave(store, [text]),
    onSuccess: (r) => {
      qc.setQueryData(['byHand'], r.lists)
      setDrafts((d) => {
        const next = { ...d }
        delete next[store]
        return next
      })
      for (const key of [['setup'], ['stats'], ['games'], keys.profile]) qc.invalidateQueries({ queryKey: key })
      const changes = [r.added ? `${r.added} added` : null, r.removed ? `${r.removed} taken out` : null].filter(Boolean).join(', ')
      toast.success(`${current?.label ?? 'List'}: ${plural(r.total, 'game')}`, changes || 'Nothing changed')
    },
    onError: (e) => toast.error("Couldn't save that list", e instanceof ApiError ? e.message : String(e)),
  })
  const counts = stores.filter((s) => s.titles.length).map((s) => `${s.label} ${s.titles.length.toLocaleString()}`)
  return (
    <Row
      name="Games added by hand"
      tag="optional"
      desc="The EA app, Ubisoft Connect, Battle.net and Xbox: their libraries need a sign-in UpNext can't use"
      right={
        <>
          {counts.length ? <Status tone="good">{counts.join(' · ')}</Status> : <Status tone="dim">None</Status>}
          <TextButton aria-expanded={open} onClick={() => setOpen((o) => !o)}>
            {open ? 'Done' : counts.length ? 'Change' : 'Add games'}
          </TextButton>
        </>
      }
    >
      {open && (
        <div className="space-y-2 pb-1">
          <Segmented
            label="Store"
            value={store}
            onChange={setStore}
            options={stores.map((s) => ({ value: s.store, label: s.label }))}
          />
          <textarea
            className="field min-h-[140px] w-full font-mono text-xs"
            aria-label={`${current?.label ?? 'Store'} games, one a line`}
            placeholder={'One game a line, as the store names it:\nDead Space\nMass Effect Legendary Edition'}
            value={text}
            onChange={(e) => setDrafts((d) => ({ ...d, [store]: e.target.value }))}
            spellCheck={false}
          />
          <div className="flex flex-wrap items-center gap-3">
            <button type="button" className="btn-outline" disabled={save.isPending || text === saved} onClick={() => save.mutate()}>
              {save.isPending ? 'Saving…' : 'Save list'}
            </button>
            <span className="text-[13px] text-dim">
              Copy the names from the store's library page. A game taken off the list leaves the library.
            </span>
          </div>
        </div>
      )}
    </Row>
  )
}
