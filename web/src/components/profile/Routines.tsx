import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { Routine, RoutineBody } from '../../types'
import { cx } from '../../lib/format'
import { DAYS, INPUTS, LENGTHS, PLACES, ROUTINE_NAME_MAX, ROUTINES_KEY, ROUTINES_MAX, TIMES, WHO, routineBody, routineSummary } from '../../lib/routines'
import { useToast } from '../useToast'
import { Section, TextButton } from '../settings/parts'
import { CHIP, CHIP_OFF, CHIP_ON } from './words'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** Routines: named sessions that put the habits together, offered on
 *  the Pick page so a pick fits who you're with and what you're holding. */
export function RoutinesSection() {
  const qc = useQueryClient()
  const toast = useToast()
  const routines = useQuery({ queryKey: ROUTINES_KEY, queryFn: api.routines })
  // The routine being edited, 'new' for the form that adds one, or null.
  const [editing, setEditing] = useState<number | 'new' | null>(null)
  const [busy, setBusy] = useState(false)
  const list = routines.data?.routines ?? []
  const refresh = () => Promise.all([qc.invalidateQueries({ queryKey: ROUTINES_KEY }), qc.invalidateQueries({ queryKey: ['taste-sources'] })])

  const save = async (id: number | null, body: RoutineBody) => {
    setBusy(true)
    try {
      const saved = id == null ? await api.addRoutine(body) : await api.saveRoutine(id, body)
      setEditing(null)
      toast.success(id == null ? `Added “${saved.name}”` : `Saved “${saved.name}”`)
    } catch (e) {
      toast.error('Couldn’t save that routine', errText(e))
    } finally {
      setBusy(false)
      await refresh()
    }
  }

  const remove = async (r: Routine) => {
    try {
      await api.deleteRoutine(r.id)
      toast.success(`Deleted “${r.name}”`, undefined, {
        label: 'Undo',
        onClick: () =>
          void api
            .addRoutine(routineBody(r))
            .catch((e) => toast.error(`Couldn’t put “${r.name}” back`, errText(e)))
            .finally(() => void refresh()),
      })
    } catch (e) {
      toast.error(`Couldn’t delete “${r.name}”`, errText(e))
    } finally {
      await refresh()
    }
  }

  return (
    <Section id="routines" title="Routines" desc="Personal. Named sessions the Pick page offers, so picks fit the night you’re having">
      <div className="mt-4 flex max-w-[860px] flex-col gap-4 text-[15.5px] leading-[1.6]">
        <p className="m-0 text-muted">
          Put together when you play, who with, where, on what and for how long, like “Friday with friends: evenings, at a desk, keyboard and mouse, two hours”.
          The Pick page shows them as chips and chooses the one that fits the time; picks then fit it, so a night with friends doesn’t get a long single-player game.
        </p>
        {routines.isError && <p className="m-0 text-sm text-bad">Couldn’t load your routines: {errText(routines.error)}</p>}
        {list.length > 0 && (
          <ul className="m-0 list-none border-t border-line p-0">
            {list.map((r) => (
              <li key={r.id} className="border-b border-line py-3">
                {editing === r.id ? (
                  <RoutineForm initial={routineBody(r)} busy={busy} onSave={(b) => void save(r.id, b)} onCancel={() => setEditing(null)} />
                ) : (
                  <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
                    <div className="min-w-0 flex-1">
                      <span className="font-bold">{r.name}</span>
                      {routines.data?.now === r.id && <span className="ml-2.5 text-[12.5px] text-accent">fits now</span>}
                      <div className="text-[13.5px] text-dim">{routineSummary(r)}</div>
                      {r.note && <div className="text-[13.5px] text-dim">“{r.note}”</div>}
                    </div>
                    <TextButton onClick={() => setEditing(r.id)}>Edit</TextButton>
                    <TextButton onClick={() => void remove(r)}>Delete</TextButton>
                  </div>
                )}
              </li>
            ))}
          </ul>
        )}
        {editing === 'new' ? (
          <RoutineForm initial={routineBody()} busy={busy} onSave={(b) => void save(null, b)} onCancel={() => setEditing(null)} />
        ) : (
          routines.data &&
          (list.length < ROUTINES_MAX ? (
            <div>
              <TextButton onClick={() => setEditing('new')}>{list.length ? 'Add a routine' : 'Add your first routine'}</TextButton>
            </div>
          ) : (
            <p className="m-0 text-[13.5px] text-dim">That’s {ROUTINES_MAX} routines, the most it keeps; delete one to add another.</p>
          ))
        )}
      </div>
    </Section>
  )
}

/** One row of chips; `multi` takes any number, otherwise tapping the chosen one clears it. */
function Chips<T extends string | number>({
  label,
  options,
  value,
  multi = false,
  onChange,
}: {
  label: string
  options: { value: T; label: string }[]
  value: T[]
  multi?: boolean
  onChange: (v: T[]) => void
}) {
  const flip = (v: T) => {
    const on = value.includes(v)
    if (multi) onChange(options.map((o) => o.value).filter((o) => (o === v ? !on : value.includes(o))))
    else onChange(on ? [] : [v])
  }
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-[13.5px] text-dim">{label}</span>
      <div role="group" aria-label={label} className="flex flex-wrap gap-2">
        {options.map((o) => (
          <button
            key={String(o.value)}
            type="button"
            aria-pressed={value.includes(o.value)}
            onClick={() => flip(o.value)}
            className={cx(CHIP, value.includes(o.value) ? CHIP_ON : CHIP_OFF)}
          >
            {o.label}
          </button>
        ))}
      </div>
    </div>
  )
}

/** Adding or changing a routine. Only the name is needed; every part left out means "any" or "not said". */
function RoutineForm({
  initial,
  busy,
  onSave,
  onCancel,
}: {
  initial: RoutineBody
  busy: boolean
  onSave: (b: RoutineBody) => void
  onCancel: () => void
}) {
  const [b, setB] = useState<RoutineBody>(initial)
  const set = (patch: Partial<RoutineBody>) => setB((cur) => ({ ...cur, ...patch }))
  const one = <T,>(v: T[]): T | null => v[0] ?? null
  const ok = b.name.trim().length > 0
  return (
    <form
      className="flex flex-col gap-4 py-2"
      aria-label={initial.name ? `Change ${initial.name}` : 'New routine'}
      onSubmit={(e) => {
        e.preventDefault()
        if (ok && !busy) onSave({ ...b, name: b.name.trim(), note: b.note?.trim() || null })
      }}
    >
      <label className="flex flex-col gap-1 text-[13.5px] text-dim">
        Name
        <input
          type="text"
          autoFocus
          value={b.name}
          maxLength={ROUTINE_NAME_MAX}
          onChange={(e) => set({ name: e.target.value })}
          onKeyDown={(e) => e.key === 'Escape' && onCancel()}
          placeholder="Friday with friends"
          className="field max-w-[420px] text-sm"
        />
      </label>
      <Chips label="Days (none for any day)" options={DAYS} value={b.days ?? []} multi onChange={(days) => set({ days })} />
      <Chips label="Times of day (none for any time)" options={TIMES} value={b.times ?? []} multi onChange={(times) => set({ times })} />
      <Chips label="Who with" options={WHO} value={b.who ? [b.who] : []} onChange={(v) => set({ who: one(v) })} />
      <Chips label="Where" options={PLACES} value={b.place ? [b.place] : []} onChange={(v) => set({ place: one(v) })} />
      <Chips label="Playing on" options={INPUTS} value={b.input ? [b.input] : []} onChange={(v) => set({ input: one(v) })} />
      <Chips label="How long" options={LENGTHS} value={b.minutes ? [b.minutes] : []} onChange={(v) => set({ minutes: one(v) })} />
      <label className="flex flex-col gap-1 text-[13.5px] text-dim">
        Anything else, in your own words
        <input
          type="text"
          value={b.note ?? ''}
          maxLength={300}
          onChange={(e) => set({ note: e.target.value })}
          placeholder="Voice chat on, two or three of us"
          className="field text-sm"
        />
      </label>
      <p className="m-0 text-[13.5px] text-dim">
        Days and times let the Pick page choose it by itself; without them it’s only chosen by hand. A Steam Deck counts as a controller.
      </p>
      <div className="flex gap-3">
        <button type="submit" className="btn-accent h-10" disabled={busy || !ok}>
          {busy ? 'Saving…' : 'Save'}
        </button>
        <button type="button" className="btn-quiet h-10" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  )
}
