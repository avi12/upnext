import type { ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router'
import { api, ApiError } from '../../api'
import { setBigScreen, setBigScreenOptions, useBigScreenOptions, type BigScreenOptions } from '../../lib/bigscreen'
import { inApp, windowAction } from '../../lib/frame'
import { steamShortcutState } from '../../lib/steamShortcut'
import { canSwitch, openComputer, remoteName } from '../../lib/connect'
import { keys, useSettings } from '../../lib/queries'

const BUTTONS: { value: BigScreenOptions['buttons']; label: string }[] = [
  { value: 'auto', label: 'The pad’s own' },
  { value: 'xbox', label: 'Xbox' },
  { value: 'playstation', label: 'PlayStation' },
  { value: 'deck', label: 'Steam Deck' },
]

function message(e: unknown): string {
  return e instanceof ApiError || e instanceof Error ? e.message : String(e)
}

function Row({ name, desc, children }: { name: string; desc?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-[1rem] border-b-[0.08rem] border-line py-[1rem]">
      <div className="min-w-0">
        <div className="text-[1.15rem] font-bold">{name}</div>
        {desc && <div className="text-[0.95rem] text-muted">{desc}</div>}
      </div>
      <div className="flex flex-wrap gap-[0.5rem]">{children}</div>
    </div>
  )
}

/** Big screen's Settings, the short list a pad needs: sync, the AI's test,
 *  big screen's own options, Add to Steam on the computer UpNext runs on,
 *  the way back to the full Settings, and in the app window, closing it
 *  (a full-screen window has no title bar to close it from). */
export function BigSettingsPage() {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const options = useBigScreenOptions()
  const sync = useMutation({
    mutationFn: () => api.startSync(),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.jobs }),
  })
  const test = useMutation({ mutationFn: api.llmTest })

  return (
    <div className="flex max-w-[56rem] flex-col pt-[1.5rem]">
      <h1 className="m-0 mb-[0.5rem] font-display text-[1.6rem] font-bold">Settings</h1>
      <Row
        name="Sync now"
        desc={sync.isSuccess ? 'Syncing your libraries. It carries on while you pick.' : sync.error ? message(sync.error) : 'Fetch what you own and play from every store.'}
      >
        <button type="button" className="big-btn-outline" data-focus-first="" disabled={sync.isPending} onClick={() => sync.mutate()}>
          {sync.isPending ? 'Starting…' : 'Sync now'}
        </button>
      </Row>
      <Row
        name="The AI"
        desc={
          test.data
            ? test.data.ok
              ? `It answered${test.data.model ? ` (${test.data.model})` : ''}.`
              : `It didn't answer: ${test.data.error ?? 'no reason given'}`
            : test.error
              ? message(test.error)
              : 'Check that picks can reach the AI.'
        }
      >
        <button type="button" className="big-btn-outline" disabled={test.isPending} onClick={() => test.mutate()}>
          {test.isPending ? 'Testing…' : 'Test'}
        </button>
      </Row>
      <Row name="Button names" desc="The names the hints show for each button.">
        {BUTTONS.map((b) => (
          <button key={b.value} type="button" className="big-chip" aria-pressed={options.buttons === b.value} onClick={() => setBigScreenOptions({ buttons: b.value })}>
            {b.label}
          </button>
        ))}
      </Row>
      <Row name="TV margin" desc="Room around the edges, for a TV that cuts them off.">
        <button type="button" className="big-chip" aria-pressed={options.overscan} onClick={() => setBigScreenOptions({ overscan: !options.overscan })}>
          {options.overscan ? 'On' : 'Off'}
        </button>
      </Row>
      <AddToSteam />
      <Row name="Leave big screen" desc="Everything else is in the full Settings, made for a mouse and keyboard.">
        <button type="button" className="big-btn-outline" onClick={() => setBigScreen(false)}>
          Leave big screen
        </button>
        <button
          type="button"
          className="big-btn-outline"
          onClick={() => {
            setBigScreen(false)
            navigate('/settings')
          }}
        >
          All settings
        </button>
      </Row>
      <OtherComputer />
      {inApp() && (
        <Row name="Close UpNext" desc="Closes this window, as its close button does.">
          <button type="button" className="big-btn-outline" onClick={() => windowAction('close')}>
            Close UpNext
          </button>
        </Row>
      )}
    </div>
  )
}

/** Which computer's library the window shows: on
 *  another computer's, the way back to this one's; on this one's, the
 *  computers on the network. Only the app window can switch. */
function OtherComputer() {
  const navigate = useNavigate()
  if (!canSwitch()) return null
  const remote = remoteName()
  return remote ? (
    <Row name="Library" desc={`This window shows ${remote}’s UpNext. Games still start on this computer.`}>
      <button type="button" className="big-btn-outline" onClick={() => openComputer(null)}>
        Use this computer’s library
      </button>
    </Row>
  ) : (
    <Row name="Library" desc="Show the UpNext on another computer on your network in this window.">
      <button type="button" className="big-btn-outline" onClick={() => navigate('/connect')}>
        Another computer
      </button>
    </Row>
  )
}

/** "Add to Steam", on the computer UpNext runs on only:
 *  it writes Steam's files there. */
function AddToSteam() {
  const qc = useQueryClient()
  const settings = useSettings({ staleTime: 10_000 })
  const here = !!settings.data?.listen.this_computer
  const shortcut = useQuery({ queryKey: ['steam-shortcut'], queryFn: api.steamShortcut, enabled: here, staleTime: 10_000 })
  const add = useMutation({
    mutationFn: api.addToSteam,
    onSuccess: (now) => qc.setQueryData(['steam-shortcut'], now),
    onSettled: () => qc.invalidateQueries({ queryKey: ['steam-shortcut'] }),
  })
  if (!here || !shortcut.data) return null
  const state = steamShortcutState(shortcut.data)
  if (shortcut.data.unavailable) return null
  return (
    <Row name="Add to Steam" desc={add.error ? message(add.error) : state.text}>
      {state.added ? (
        <span className="text-[1rem] text-good">Added</span>
      ) : (
        <button type="button" className="big-btn-outline" disabled={!state.canAdd || add.isPending} onClick={() => add.mutate()}>
          {add.isPending ? 'Adding…' : 'Add to Steam'}
        </button>
      )}
    </Row>
  )
}
