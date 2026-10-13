import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../../api'
import type { Settings } from '../../types'
import { setBigScreen } from '../../lib/bigscreen'
import { LAUNCHER_STEPS, steamShortcutState } from '../../lib/steamShortcut'
import { CopyLines } from '../setup/parts'
import { useToast } from '../useToast'
import { Row, RowGrid, Section } from './parts'

/** Settings › Big screen: turning it on, and adding it to Steam and
 *  the other launchers a handheld opens on. Adding is for the computer
 *  UpNext runs on: it writes Steam's files there. */
export function BigScreenSection({ s }: { s: Settings }) {
  const here = !!s.listen.this_computer
  const shortcut = useQuery({ queryKey: ['steam-shortcut'], queryFn: api.steamShortcut, enabled: here, staleTime: 10_000 })
  const target = shortcut.data?.target
  return (
    <Section id="big-screen" title="Big screen" desc="For a controller, a handheld or a TV.">
      <RowGrid>
        <Row
          name="Big screen"
          wrap
          desc="Large type and one focus that a gamepad, arrow keys or a TV remote moves. Leave it from its Settings or the Buttons help."
          right={
            <button type="button" className="btn-outline" onClick={() => setBigScreen(true)}>
              Switch to big screen
            </button>
          }
        />
        {here && <AddToSteam />}
      </RowGrid>
      {here && target && (
        <Row
          name="Other launchers"
          wrap
          desc={
            s.os === 'windows'
              ? 'A handheld’s own launcher can start UpNext in big screen too. None of them lets a program add itself, so add it there by hand.'
              : 'Any launcher that adds a program can start UpNext in big screen with these.'
          }
        >
          <div className="flex max-w-[760px] flex-col gap-3">
            {s.os === 'windows' && (
              <ul className="m-0 flex list-none flex-col gap-2 p-0 text-[14px]">
                {LAUNCHER_STEPS.map((l) => (
                  <li key={l.name}>
                    <span className="font-semibold">{l.name}:</span> <span className="text-muted">{l.steps}</span>
                  </li>
                ))}
              </ul>
            )}
            <CopyLines label="Program" text={target.program} />
            <CopyLines label="Arguments" text={target.arguments} />
          </div>
        </Row>
      )}
    </Section>
  )
}

function AddToSteam() {
  const qc = useQueryClient()
  const toast = useToast()
  const shortcut = useQuery({ queryKey: ['steam-shortcut'], queryFn: api.steamShortcut, staleTime: 10_000 })
  const add = useMutation({
    mutationFn: api.addToSteam,
    onSuccess: (now) => {
      qc.setQueryData(['steam-shortcut'], now)
      toast.success('Added to Steam', 'UpNext is in its library under Non-Steam, and opens in big screen')
    },
    onError: (e) => {
      void qc.invalidateQueries({ queryKey: ['steam-shortcut'] })
      toast.error("Couldn't add UpNext to Steam", e instanceof Error ? e.message : String(e))
    },
  })
  const state = shortcut.data ? steamShortcutState(shortcut.data) : null
  return (
    <Row
      name="Add to Steam"
      wrap
      desc={shortcut.isError ? `Couldn't look: ${shortcut.error.message}` : (state?.text ?? 'Looking for Steam…')}
      right={
        state &&
        (state.added ? (
          <span className="text-[14px] text-good">Added</span>
        ) : (
          <button type="button" className="btn-outline" disabled={!state.canAdd || add.isPending} onClick={() => add.mutate()}>
            {add.isPending ? 'Adding…' : 'Add to Steam'}
          </button>
        ))
      }
    />
  )
}
