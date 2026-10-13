import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '../../api'
import type { GameView, TasteChoice, TasteFavourite } from '../../types'
import { cx, minutesToText } from '../../lib/format'
import { useDebounced } from '../../lib/useDebounced'
import { Cover } from '../Cover'
import { Section, TextButton } from '../settings/parts'
import { andList, CHIP, CHIP_OFF, CHIP_ON } from './words'

/** Favourites: each with why, which you can edit or remove, and a way to add more. */
export function FavouritesSection({
  favourites,
  suggestions,
  onOpen,
  onAdd,
  onSave,
  onRemove,
}: {
  favourites: TasteFavourite[]
  suggestions: GameView[]
  onOpen: (g: GameView) => void
  /** Resolves to the new favourite's id, or null when it failed. */
  onAdd: (g: GameView) => Promise<number | null>
  onSave: (f: TasteFavourite, why: string[], note: string) => Promise<boolean>
  onRemove: (f: TasteFavourite) => void
}) {
  const [editing, setEditing] = useState<number | null>(null)
  const [adding, setAdding] = useState(false)

  const add = async (g: GameView) => {
    const id = await onAdd(g)
    if (id != null) setEditing(id)
  }

  return (
    <Section id="favourites" title="Favourites" desc="Games you love, and why. They never fade.">
      {favourites.length === 0 && (
        <p className="m-0 mt-4 max-w-[760px] text-[15.5px] leading-[1.6] text-muted">
          None yet. Choose a few games you love and say what you love about them; a vague request like “anything” leans on them most.
        </p>
      )}
      <ul className="m-0 list-none p-0 xl:grid xl:grid-cols-2 xl:gap-x-14">
        {favourites.map((f) => (
          <FavouriteRow
            key={f.id}
            fav={f}
            editing={editing === f.id}
            onEdit={() => setEditing(editing === f.id ? null : f.id)}
            onOpen={onOpen}
            onSave={async (why, note) => {
              const ok = await onSave(f, why, note)
              if (ok) setEditing(null)
            }}
            onRemove={() => onRemove(f)}
          />
        ))}
      </ul>

      <div className="mt-6 flex flex-col gap-3">
        {suggestions.length > 0 && (
          <>
            <p className="m-0 text-[15px] text-muted">
              {favourites.length ? 'More from your most played' : 'From your most played, recent play first'}. Press one to make it a favourite.
            </p>
            <ul aria-label="Your most played" className="m-0 grid list-none grid-cols-3 gap-4 p-0 sm:grid-cols-6">
              {suggestions.slice(0, favourites.length ? 6 : 12).map((g) => (
                <li key={g.id}>
                  <button type="button" onClick={() => void add(g)} className="group flex w-full flex-col gap-1.5 text-left" aria-label={`Make ${g.name} a favourite`}>
                    <Cover game={g} className="w-full rounded-[8px] transition group-hover:opacity-80" titleSize="text-[11px]" />
                    <span className="truncate text-[13px] text-ink">{g.name}</span>
                    <span className="mono -mt-1">{minutesToText(g.playtime_minutes)}</span>
                  </button>
                </li>
              ))}
            </ul>
          </>
        )}
        {adding ? (
          <FindGame exclude={new Set(favourites.map((f) => f.game.id))} onPick={(g) => void add(g)} onClose={() => setAdding(false)} />
        ) : (
          <TextButton className="self-start" onClick={() => setAdding(true)}>
            Find another game
          </TextButton>
        )}
      </div>
    </Section>
  )
}

function FavouriteRow({
  fav,
  editing,
  onEdit,
  onOpen,
  onSave,
  onRemove,
}: {
  fav: TasteFavourite
  editing: boolean
  onEdit: () => void
  onOpen: (g: GameView) => void
  onSave: (why: string[], note: string) => Promise<void>
  onRemove: () => void
}) {
  const g = fav.game
  const why = fav.why.map((c) => c.label)
  return (
    <li className="border-b border-line py-4">
      <div className="flex flex-wrap items-start gap-x-5 gap-y-2">
        <button type="button" onClick={() => onOpen(g)} className="shrink-0 rounded-[8px]" aria-label={`Open ${g.name}`}>
          <Cover game={g} className="w-14 rounded-[8px]" titleSize="text-[9px]" />
        </button>
        <div className="flex min-w-0 flex-1 basis-56 flex-col gap-1">
          <button
            type="button"
            onClick={() => onOpen(g)}
            className="self-start text-left font-display text-[18px] leading-[1.2] font-bold tracking-[-0.01em] hover:text-accent"
          >
            {g.name}
          </button>
          <p className="m-0 text-[14.5px] leading-[1.55] text-muted">
            {why.length ? `For ${andList(why)}.` : 'You haven’t said why yet.'}
            {fav.note && <span className="text-ink"> “{fav.note}”</span>}
          </p>
        </div>
        <div className="flex items-center gap-x-4">
          <TextButton onClick={onEdit} aria-expanded={editing}>
            {editing ? 'Close' : why.length || fav.note ? 'Change why' : 'Say why'}
          </TextButton>
          <TextButton onClick={onRemove}>Remove</TextButton>
        </div>
      </div>
      {editing && <WhyEditor fav={fav} onSave={onSave} />}
    </li>
  )
}

/** The reasons a game offers as chips, any already chosen first, and a line of your own. */
function WhyEditor({ fav, onSave }: { fav: TasteFavourite; onSave: (why: string[], note: string) => Promise<void> }) {
  const [chosen, setChosen] = useState<string[]>(fav.why.map((c) => c.key))
  const [note, setNote] = useState(fav.note ?? '')
  const [busy, setBusy] = useState(false)
  const all: TasteChoice[] = [...fav.why, ...fav.choices.filter((c) => !fav.why.some((w) => w.key === c.key))]
  const flip = (key: string) => setChosen((c) => (c.includes(key) ? c.filter((k) => k !== key) : [...c, key]))
  const save = async () => {
    setBusy(true)
    try {
      await onSave(chosen, note)
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="mt-3 flex max-w-[860px] flex-col gap-3 pl-0 sm:pl-[76px]">
      {all.length > 0 ? (
        <div role="group" aria-label={`What you love about ${fav.game.name}`} className="flex flex-wrap gap-2">
          {all.map((c) => (
            <button key={c.key} type="button" aria-pressed={chosen.includes(c.key)} onClick={() => flip(c.key)} className={cx(CHIP, chosen.includes(c.key) ? CHIP_ON : CHIP_OFF)}>
              {c.label}
            </button>
          ))}
        </div>
      ) : (
        <p className="m-0 text-sm text-dim">UpNext has no tags or review summary for this game yet, so say it in your own words.</p>
      )}
      <label className="flex flex-col gap-1 text-[13.5px] text-dim">
        In your own words
        <input
          type="text"
          value={note}
          maxLength={300}
          onChange={(e) => setNote(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && void save()}
          placeholder="The last hour, the soundtrack, playing it with my brother…"
          className="field text-sm"
        />
      </label>
      <div className="flex gap-3">
        <button type="button" className="btn-accent h-10" disabled={busy} onClick={() => void save()}>
          {busy ? 'Saving…' : 'Save'}
        </button>
      </div>
    </div>
  )
}

/** A search of the library for a game to add. */
export function FindGame({ exclude, onPick, onClose }: { exclude: Set<number>; onPick: (g: GameView) => void; onClose: () => void }) {
  const [q, setQ] = useState('')
  const term = useDebounced(q.trim(), 200)
  const found = useQuery({
    queryKey: ['games', 'find-favourite', term],
    queryFn: () => api.games({ q: term, limit: 8 }),
    enabled: term.length > 1,
  })
  const items = (found.data?.items ?? []).filter((g) => !exclude.has(g.id))
  return (
    <div className="flex max-w-[620px] flex-col gap-2">
      <div className="flex items-center gap-3">
        <input
          type="search"
          autoFocus
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => e.key === 'Escape' && onClose()}
          placeholder="Find a game you love…"
          aria-label="Find a game to make a favourite"
          className="field min-w-0 flex-1 text-sm"
        />
        <TextButton onClick={onClose}>Done</TextButton>
      </div>
      {term.length > 1 && found.data && items.length === 0 && <p className="m-0 text-sm text-dim">Nothing in your library matches “{term}”.</p>}
      <ul className="m-0 list-none p-0">
        {items.map((g) => (
          <li key={g.id}>
            <button type="button" onClick={() => onPick(g)} className="flex w-full items-center gap-3 rounded-md px-1 py-1.5 text-left hover:bg-raised">
              <Cover game={g} className="w-8 rounded-[5px]" titleSize="text-[7px]" />
              <span className="min-w-0 flex-1 truncate text-[14.5px]">{g.name}</span>
              <span className="mono">{g.playtime_minutes > 0 ? minutesToText(g.playtime_minutes) : 'not played'}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}
