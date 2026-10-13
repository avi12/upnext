import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import { cx } from '../../lib/format'
import { keys } from '../../lib/queries'
import { useToast } from '../useToast'
import { Ext, Row, Status, TextButton } from './parts'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** The Epic sign-in, for Epic games without Heroic. Epic's sign-in page
 *  ends on a page of JSON holding a one-time code, which is pasted here.
 *  `launcherDir` is the Epic Games Launcher's manifests folder, when it's on
 *  this computer: its installs come in without a sign-in. */
export function EpicRow({ launcherDir }: { launcherDir?: string | null }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [open, setOpen] = useState(false)
  const [code, setCode] = useState('')
  const epic = useQuery({ queryKey: ['epic'], queryFn: api.epic, staleTime: 60_000 })
  const connected = epic.data?.connected === true

  const invalidate = () => {
    for (const key of [keys.profile, keys.settings, keys.config, ['setup']]) qc.invalidateQueries({ queryKey: key })
  }
  const login = useMutation({
    mutationFn: (c: string) => api.epicLogin(c),
    onSuccess: (st) => {
      qc.setQueryData(['epic'], st)
      invalidate()
      setCode('')
      setOpen(false)
      toast.success('Epic connected', st.display_name ? `Signed in as ${st.display_name}` : undefined)
    },
  })
  const logout = useMutation({
    mutationFn: api.epicLogout,
    onSuccess: (st) => {
      qc.setQueryData(['epic'], st)
      invalidate()
      toast.info('Epic disconnected')
    },
    onError: (e) => toast.error("Couldn't disconnect", errText(e)),
  })

  let right
  if (epic.isLoading) right = <Status tone="dim">Checking…</Status>
  else if (epic.isError) right = <Status tone="dim">Status unavailable</Status>
  else if (connected)
    right = (
      <>
        <Status tone="good">{epic.data?.display_name ?? 'Connected'}</Status>
        <TextButton disabled={logout.isPending} onClick={() => logout.mutate()}>
          Disconnect
        </TextButton>
      </>
    )
  else
    right = (
      <>
        <Status tone={epic.data?.error ? 'bad' : 'dim'} title={epic.data?.error ?? undefined}>
          {epic.data?.error ? 'Signed out' : 'Not connected'}
        </Status>
        <TextButton aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          {open ? 'Hide' : 'Connect'}
        </TextButton>
      </>
    )

  const desc = launcherDir ? (
    <span title={launcherDir}>
      Your Epic games and playtime · the Epic Games Launcher's installs from{' '}
      <span className="font-mono text-[11.5px]">{launcherDir}</span>
    </span>
  ) : (
    'Your Epic games and playtime, without Heroic'
  )

  return (
    <Row name="Epic Games" tag="optional" desc={desc} right={right}>
      {!connected && epic.data?.error && !open && (
        <p className="m-0 pb-1 text-[13px] leading-relaxed text-dim">{epic.data.error}</p>
      )}
      {!connected && open && (
        <div className="space-y-2 pb-1">
          <p className="m-0 text-[13px] leading-relaxed text-dim">
            Sign in on Epic's own {epic.data?.auth_url ? <Ext href={epic.data.auth_url}>sign-in page</Ext> : 'sign-in page'}. It
            ends on a page of text with an <span className="font-mono text-[12px]">authorizationCode</span>: paste the code, or the
            whole page, here within a few minutes. UpNext never sees your password.
          </p>
          <form
            className="flex flex-wrap items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault()
              if (code.trim()) login.mutate(code.trim())
            }}
          >
            <input
              className={cx('field min-w-0 flex-1 font-mono text-xs', login.isError && 'border-bad')}
              placeholder="Paste the code or the page"
              aria-label="Epic authorization code"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <button type="submit" className="btn-outline" disabled={!code.trim() || login.isPending}>
              {login.isPending ? 'Connecting…' : 'Connect'}
            </button>
          </form>
          {login.isError && (
            <p role="alert" className="m-0 text-[12.5px] text-bad">
              {errText(login.error)}
            </p>
          )}
        </div>
      )}
    </Row>
  )
}
