import { useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { BackupKind, Job, JobsResponse } from '../../types'
import { useToast } from '../useToast'
import { absoluteTime, cx, relativeTime, upperFirst } from '../../lib/format'
import { isPast } from '../../lib/jobs'
import { Section, TextButton } from './parts'
import { restoreProblem } from './backupText'
import { keys, useJobs } from '../../lib/queries'

/** What a backup's kind says when UpNext gave no reason of its own. */
const KIND: Record<BackupKind, string> = {
  scheduled: 'Backup job',
  manual: 'By hand',
  upgrade: 'Before an upgrade',
  restore: 'Before a restore',
  import: 'Before an import',
}

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** "39 MB", "1.2 MB" */
function sizeText(bytes: number) {
  const mb = bytes / 1e6
  return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`
}

type Open = { name: string; what: 'download' | 'restore' | 'delete' }

/** Settings › Backups: back up now, the folder's backups with downloads, restore and delete, and importing a database. */
export function BackupsSection() {
  const qc = useQueryClient()
  const toast = useToast()
  const [note, setNote] = useState('')
  const [open, setOpen] = useState<Open | null>(null)
  const [file, setFile] = useState<File | null>(null)
  const fileInput = useRef<HTMLInputElement>(null)

  const backups = useQuery({ queryKey: ['backups'], queryFn: api.backups, refetchInterval: 60_000 })
  const jobs = useJobs({ refetchInterval: 10_000 })
  const running = jobs.data?.current?.status === 'running' ? jobs.data.current : null
  const busy = running ? 'Another job is running' : undefined

  const onStarted = (j: Job) => {
    qc.setQueryData<JobsResponse>(keys.jobs, (old) => ({ current: j, recent: old?.recent ?? [] }))
    setOpen(null)
    setFile(null)
  }
  const backUp = useMutation({
    mutationFn: () => api.backUpNow(note.trim() || undefined),
    onSuccess: (b) => {
      setNote('')
      qc.invalidateQueries({ queryKey: ['backups'] })
      toast.success('Backed up', b.name)
    },
    onError: (e) => toast.error("Couldn't back up", errText(e)),
  })
  const restore = useMutation({
    mutationFn: api.restoreBackup,
    onSuccess: onStarted,
    onError: (e) => toast.error("Couldn't restore that", errText(e)),
  })
  const importDb = useMutation({
    mutationFn: api.importDatabase,
    onSuccess: onStarted,
    onError: (e) => toast.error("Couldn't import that", errText(e)),
  })
  const remove = useMutation({
    mutationFn: api.deleteBackup,
    onSuccess: () => {
      setOpen(null)
      qc.invalidateQueries({ queryKey: ['backups'] })
    },
    onError: (e) => toast.error("Couldn't delete it", errText(e)),
  })

  const data = backups.data
  const list = data?.backups ?? []
  const toggle = (name: string, what: Open['what']) => setOpen((cur) => (cur?.name === name && cur.what === what ? null : { name, what }))
  const replaces = 'the library, statuses, lists, picks, settings and job history'

  return (
    <Section id="backups" title="Backups" desc="Copies of the whole database. A restore or an import backs up what it replaces first.">
      <div className="flex flex-wrap items-end gap-x-3 gap-y-2 border-b border-line py-3.5">
        <label className="flex min-w-0 flex-1 basis-[260px] flex-col gap-1 text-[13.5px] text-dim">
          Note, if you like
          <input
            className="field text-sm"
            value={note}
            maxLength={300}
            placeholder="before trying the new prompt"
            onChange={(e) => setNote(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !backUp.isPending) backUp.mutate()
            }}
          />
        </label>
        <button type="button" className="btn-outline" disabled={backUp.isPending} onClick={() => backUp.mutate()}>
          {backUp.isPending ? 'Backing up…' : 'Back up now'}
        </button>
        <p className="m-0 w-full text-[13.5px] text-dim">
          Kept until you delete it. The backup job under Jobs makes its own and keeps the newest {data?.keep_scheduled ?? 4}.
        </p>
      </div>

      <div className="border-b border-line py-3.5">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <div className="min-w-0 flex-1 basis-[260px]">
            <div className="text-[15.5px] font-bold">Import a database</div>
            <div className="text-[13.5px] text-dim">An UpNext database from another computer or an older install.</div>
          </div>
          <input
            ref={fileInput}
            type="file"
            accept=".db,.sqlite,.sqlite3,application/vnd.sqlite3,application/x-sqlite3"
            className="sr-only"
            tabIndex={-1}
            aria-hidden
            onChange={(e) => {
              setFile(e.target.files?.[0] ?? null)
              setOpen(null)
              e.target.value = ''
            }}
          />
          <button type="button" className="btn-outline" disabled={!!running || importDb.isPending} title={busy} onClick={() => fileInput.current?.click()}>
            Choose a file…
          </button>
        </div>
        {file && (
          <Confirm
            text={
              <>
                Import <b className="font-semibold text-ink">{file.name}</b> ({sizeText(file.size)})? It replaces {replaces}. The database as it
                is now is backed up first. Keys and sign-ins the file doesn't have stay as they are.
              </>
            }
            warning="Its schedules come too. If they're on, jobs they have due start within a minute, such as a sync if its last one was over a day ago."
            action={importDb.isPending ? 'Importing…' : 'Import'}
            disabled={!!running || importDb.isPending}
            title={busy}
            onConfirm={() => importDb.mutate(file)}
            onCancel={() => setFile(null)}
          />
        )}
      </div>

      {backups.isError && <p className="py-3 text-sm text-bad">Couldn't load the backups: {errText(backups.error)}</p>}
      {data && list.length === 0 && <p className="m-0 border-b border-line py-3 text-sm text-dim">No backups yet.</p>}
      {list.length > 0 && (
        <ul className="m-0 list-none p-0">
          {list.map((b) => {
            const shown = open?.name === b.name ? open.what : null
            const problem = restoreProblem(b, data!.schema, data!.oldest)
            return (
              <li key={b.name} className="border-b border-line py-3">
                <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
                  <div className="min-w-0 flex-1 basis-[300px]">
                    <div className="text-[15px]">
                      <span className="font-semibold">{b.reason ?? KIND[b.kind]}</span>
                      {b.note && <span className="text-muted"> · “{b.note}”</span>}
                    </div>
                    <div className="text-[13.5px] text-dim">
                      <span title={absoluteTime(b.made_at)}>{relativeTime(b.made_at)}</span>
                      {' · '}
                      <span className={cx(problem && 'text-warn')} title={problem ?? undefined}>
                        {b.schema == null ? 'not a database' : `schema v${b.schema}`}
                      </span>
                      {' · '}
                      {sizeText(b.bytes)}
                    </div>
                    <div className="mono truncate" title={b.name}>
                      {b.name}
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5">
                    <TextButton aria-expanded={shown === 'download'} aria-label={`Download ${b.name}`} onClick={() => toggle(b.name, 'download')}>
                      Download
                    </TextButton>
                    <TextButton
                      aria-expanded={shown === 'restore'}
                      aria-label={`Restore ${b.name}`}
                      disabled={!!problem || !!running}
                      title={problem ?? busy}
                      onClick={() => toggle(b.name, 'restore')}
                    >
                      Restore
                    </TextButton>
                    <TextButton aria-expanded={shown === 'delete'} aria-label={`Delete ${b.name}`} onClick={() => toggle(b.name, 'delete')}>
                      Delete
                    </TextButton>
                  </div>
                </div>
                {shown === 'download' && (
                  <div className="flex flex-col gap-2.5 pt-3">
                    <Choice href={api.backupUrl(b.name, false)} label="Without keys and sign-ins">
                      Safe to share or attach to a bug report.
                    </Choice>
                    <Choice href={api.backupUrl(b.name, true)} label="Everything">
                      Includes your API keys and Steam sign-in. Keep it to yourself.
                    </Choice>
                  </div>
                )}
                {shown === 'restore' && (
                  <Confirm
                    text={
                      <>
                        Restore the backup from {relativeTime(b.made_at)}? {upperFirst(replaces)} go back to how they were then. The database as it
                        is now is backed up first, so you can undo this. Keys and sign-ins the backup doesn't have stay as they are.
                      </>
                    }
                    warning={`Its schedules come back too. If they're on, jobs they have due start within a minute${
                      isPast(b.made_at + 86_400) ? ', including a sync, since this backup is over a day old' : ''
                    }.`}
                    action={restore.isPending ? 'Restoring…' : 'Restore'}
                    disabled={!!running || restore.isPending}
                    title={busy}
                    onConfirm={() => restore.mutate(b.name)}
                    onCancel={() => setOpen(null)}
                  />
                )}
                {shown === 'delete' && (
                  <Confirm
                    text={<>Delete {b.name}? This can't be undone.</>}
                    action={remove.isPending ? 'Deleting…' : 'Delete'}
                    danger
                    disabled={remove.isPending}
                    onConfirm={() => remove.mutate(b.name)}
                    onCancel={() => setOpen(null)}
                  />
                )}
              </li>
            )
          })}
        </ul>
      )}
      {data && (
        <p className="m-0 pt-3 text-[13.5px] text-dim">
          UpNext keeps the backup job's newest {data.keep_scheduled} and the newest {data.keep_automatic} it makes before each upgrade, restore
          or import. Backups made by hand stay until you delete them. They're in <span className="font-mono text-[12px] break-all">{data.folder}</span>.{' '}
          <a href="/api/v1/export" download className="text-muted underline decoration-underline underline-offset-4 hover:text-ink">
            Download the database as it is now
          </a>
          , without keys and sign-ins.
        </p>
      )}
    </Section>
  )
}

/** A download, with what it holds. */
function Choice({ href, label, children }: { href: string; label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
      <a href={href} download className="btn-outline min-w-[220px]">
        {label}
      </a>
      <span className="text-[13.5px] text-dim">{children}</span>
    </div>
  )
}

/** A sentence that asks, with what to watch for, the action and Cancel. */
function Confirm({
  text,
  warning,
  action,
  danger,
  disabled,
  title,
  onConfirm,
  onCancel,
}: {
  text: React.ReactNode
  warning?: string
  action: string
  danger?: boolean
  disabled?: boolean
  title?: string
  onConfirm: () => void
  onCancel: () => void
}) {
  return (
    <div className="flex flex-col gap-2.5 pt-3">
      <p className="m-0 max-w-[760px] text-[14px] text-muted">{text}</p>
      {warning && <p className="m-0 max-w-[760px] text-[14px] text-warn">{warning}</p>}
      <div className="flex items-center gap-2">
        <button type="button" className={cx('btn-outline', danger && 'text-bad')} disabled={disabled} title={title} onClick={onConfirm}>
          {action}
        </button>
        <TextButton onClick={onCancel}>Cancel</TextButton>
      </div>
    </div>
  )
}
