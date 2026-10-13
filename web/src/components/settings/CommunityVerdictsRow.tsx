import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, api } from '../../api'
import { useToast } from '../useToast'
import { snapshotDay } from '../../lib/verdicts'
import { Checkbox, Row } from './parts'
import { useSettingsSave } from './useSettingsSave'
import { keys, useSettings } from '../../lib/queries'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** "Backlog verdicts from other players": the switch,
 *  the snapshot kept, a download now, and whether the picks AI reads them
 *  too (on by default since the A/B). */
export function CommunityVerdictsRow() {
  const qc = useQueryClient()
  const toast = useToast()
  const save = useSettingsSave()
  const settings = useSettings()
  const status = useQuery({ queryKey: ['verdicts-community'], queryFn: api.communityVerdicts, staleTime: 30_000 })
  const download = useMutation({
    mutationFn: () => api.runTask('verdicts'),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: keys.jobs })
      toast.success('Downloading', "Other players' backlog verdicts")
    },
    onError: (e) => toast.error("Couldn't start the download", errText(e)),
  })

  const s = settings.data
  const st = status.data
  if (!s) return null
  const on = s.verdicts_receive
  const snap = st?.snapshot
  const kept =
    snap?.date != null ? (
      <span className="text-[13.5px] font-normal text-dim">
        · {snap.matched.toLocaleString()} of your games, updated {snapshotDay(snap.date)}
      </span>
    ) : null

  const turn = async (v: boolean) => {
    const failed = await save({ verdicts_receive: v }, v ? "Other players' verdicts are on" : "Other players' verdicts are off")
    if (!failed && v && st?.service) download.mutate()
  }

  return (
    <Row
      name={<>Backlog verdicts from other players {on && kept}</>}
      desc={
        on && st && !st.service
          ? 'On. No verdicts service is set yet, so there is nothing to download.'
          : 'What other UpNext players say about how games fit a backlog, on game pages, pick cards and the library. A daily download that sends nothing about you; the last one works offline.'
      }
      wrap
      right={
        <Checkbox checked={on} onChange={(v) => void turn(v)}>
          {on ? 'On' : 'Off'}
        </Checkbox>
      }
    >
      {on && (
        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <button type="button" className="btn-outline" disabled={!st?.service || download.isPending} onClick={() => download.mutate()}>
              Download now
            </button>
            {snap?.source && <span className="text-[12.5px] text-dim">From {snap.source}</span>}
          </div>
          <Checkbox
            checked={s.verdicts_prompt}
            onChange={(v) => void save({ verdicts_prompt: v }, v ? 'The AI reads them' : "The AI doesn't read them")}
            note="one switch for all the AI is given of them, in picks, “It’s too…”, Help me decide and Second Chance"
          >
            Let the AI read them
          </Checkbox>
        </div>
      )}
    </Row>
  )
}
