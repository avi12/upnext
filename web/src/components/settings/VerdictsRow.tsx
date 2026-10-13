import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, api } from '../../api'
import { useToast } from '../useToast'
import { Checkbox, InlineField, Row } from './parts'
import { useSettingsSave } from './useSettingsSave'
import { useSettings } from '../../lib/queries'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** Backlog verdicts: the service they're sent to, whether the game
 *  page offers one after a mark, and taking them all down. */
export function VerdictsRow() {
  const qc = useQueryClient()
  const toast = useToast()
  const save = useSettingsSave()
  const [open, setOpen] = useState(false)
  const [sure, setSure] = useState(false)
  const settings = useSettings()
  const overview = useQuery({ queryKey: ['verdicts'], queryFn: api.verdicts, staleTime: 30_000 })
  const changed = () => {
    qc.invalidateQueries({ queryKey: ['verdicts'] })
    qc.invalidateQueries({ queryKey: ['verdict'] })
  }
  const refresh = useMutation({
    mutationFn: api.refreshVerdicts,
    onSuccess: (r) => {
      changed()
      toast.success('Checked the service', `It has ${r.listed} of yours; ${r.added} added here and ${r.changed} changed.`)
    },
    onError: (e) => toast.error("Couldn't check", errText(e)),
  })
  const takeAll = useMutation({
    mutationFn: api.withdrawAllVerdicts,
    onSuccess: (r) => {
      changed()
      setSure(false)
      toast.success('Taken down', `${r.withdrawn} ${r.withdrawn === 1 ? 'verdict' : 'verdicts'}${r.forgot_key ? '; your key is forgotten' : ''}.`)
    },
    onError: (e) => toast.error("Couldn't take them down", errText(e)),
  })

  const s = settings.data
  const o = overview.data
  const out = o?.verdicts.filter((v) => v.state === 'sent' || v.state === 'held').length ?? 0
  const drafts = o?.verdicts.filter((v) => v.state === 'draft' || v.state === 'queued').length ?? 0
  const summary = o && (
    <span className="text-[13.5px] font-normal text-dim">
      · {out} sent{drafts > 0 ? `, ${drafts} not sent` : ''}
    </span>
  )

  return (
    <Row
      name={<>Backlog verdicts {summary}</>}
      desc="Short public answers about how a game went, shared under CC0 with other backlog players. Nothing is sent until you press Send."
      wrap
      right={
        <button type="button" className="btn-outline" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
          {open ? 'Close' : 'Change'}
        </button>
      }
    >
      {open && s && (
        <div className="flex flex-col gap-3">
          {/* Each player's key goes to this address, so only the computer
              UpNext runs on can change it. */}
          {s.listen.this_computer ? (
            <InlineField
              label="Service address"
              value={s.verdicts_url ?? ''}
              placeholder="empty: UpNext's own service"
              disabled={s.verdicts_url_source === 'env'}
              onCommit={(v) => save({ verdicts_url: v }, 'Verdicts service', { inline: true })}
              wrapClassName="max-w-xl"
              className="flex-1"
            />
          ) : (
            <p className="m-0 text-[13.5px] text-dim">
              Service address: <span className="text-ink">{s.verdicts_url ?? 'none, so nothing is sent'}</span>. It's set on the computer UpNext runs on.
            </p>
          )}
          {s.verdicts_url_source === 'env' && <p className="m-0 text-[12.5px] text-dim">Set by UPNEXT_VERDICTS_URL.</p>}
          <Checkbox checked={s.verdicts_ask} onChange={(v) => void save({ verdicts_ask: v }, v ? 'The game page offers a verdict' : "The game page won't offer one")}>
            Offer a verdict after I mark a game finished, shelved, abandoned, burnt out or not for me
          </Checkbox>
          {o && (o.verdicts.length > 0 || o.has_key) && (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <button type="button" className="btn-outline" disabled={!o.service || refresh.isPending} onClick={() => refresh.mutate()}>
                {refresh.isPending ? 'Checking…' : 'Check what the service has'}
              </button>
              {sure ? (
                <>
                  <span className="text-[13.5px] text-muted">Take every verdict down and forget your key?</span>
                  <button type="button" className="btn-outline text-bad" disabled={takeAll.isPending} onClick={() => takeAll.mutate()}>
                    Take them all down
                  </button>
                  <button type="button" className="btn-quiet h-9 px-2 text-[13.5px]" onClick={() => setSure(false)}>
                    Keep them
                  </button>
                </>
              ) : (
                <button type="button" className="btn-outline" disabled={!o.service} onClick={() => setSure(true)}>
                  Take down all my verdicts
                </button>
              )}
            </div>
          )}
        </div>
      )}
    </Row>
  )
}
