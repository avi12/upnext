import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { AiAccount, Player, Sharing, StoreAccount } from '../../types'
import { aiName } from '../../lib/ai'
import { storeName } from '../../lib/format'
import { possessive, usePlayers } from '../../lib/players'
import { useToast } from '../useToast'
import { Checkbox, Row, Section, Segmented, Status, TextButton } from './parts'
import { keys } from '../../lib/queries'

const SHARING: { value: Sharing; label: string }[] = [
  { value: 'everyone', label: 'Everyone' },
  { value: 'chosen', label: 'Chosen players' },
  { value: 'owner', label: 'Only me' },
]

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

function gamesText(n: number) {
  return `${n.toLocaleString()} ${n === 1 ? 'game' : 'games'}`
}

/** Settings › Sharing: who your store and AI accounts are offered
 *  to and who also plays on them, and what the others offer you. Each
 *  player chooses: an offered account is theirs to use or leave. */
export function SharingSection() {
  const { players, current } = usePlayers()
  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const ai = useQuery({ queryKey: ['ai-accounts'], queryFn: api.aiAccounts })
  const others = players.filter((p) => p.id !== current?.id)
  const mine = accounts.data?.accounts.filter((a) => a.mine) ?? []
  const offered = accounts.data?.accounts.filter((a) => !a.mine) ?? []
  const myAi = ai.data?.filter((a) => a.mine) ?? []
  const offeredAi = ai.data?.filter((a) => !a.mine) ?? []
  const noAi = ai.data != null && !ai.data.some((a) => a.in_use)

  return (
    <Section id="sharing" title="Sharing" desc="Games are shared, not hours: a game’s hours count for whoever plays on its account.">
      {(accounts.isError || ai.isError) && (
        <p className="py-3 text-sm text-bad">Couldn't load the accounts: {errText(accounts.error ?? ai.error)}</p>
      )}
      <h3 className="m-0 pt-4 text-[13px] font-semibold tracking-[0.06em] text-dim uppercase">Your store accounts</h3>
      {accounts.data && mine.length === 0 && (
        <p className="m-0 border-b border-line py-3 text-sm text-dim">None yet: link one under Accounts and it’s offered to everyone.</p>
      )}
      {mine.map((a) => (
        <MyStoreAccount key={a.id} account={a} others={others} />
      ))}

      <h3 className="m-0 pt-6 text-[13px] font-semibold tracking-[0.06em] text-dim uppercase">Your AI</h3>
      {myAi.map((a) => (
        <MyAiAccount key={a.id} account={a} others={others} />
      ))}
      {ai.data && myAi.length === 0 && (
        <p className="m-0 border-b border-line py-3 text-sm text-dim">None of your own: setting one up under Picks makes one.</p>
      )}

      <h3 className="m-0 pt-6 text-[13px] font-semibold tracking-[0.06em] text-dim uppercase">Shared with you</h3>
      {accounts.data && ai.data && offered.length === 0 && offeredAi.length === 0 && (
        <p className="m-0 border-b border-line py-3 text-sm text-dim">Nobody offers you anything yet.</p>
      )}
      {offered.map((a) => (
        <OfferedStoreAccount key={a.id} account={a} />
      ))}
      {offeredAi.map((a) => (
        <OfferedAiAccount key={a.id} account={a} />
      ))}
      {noAi && (
        <p className="m-0 max-w-[760px] pt-3 text-[13.5px] text-dim">
          You have no AI in use, so your picks come from UpNext’s own ranking. Set one up under Picks, or ask someone to share theirs.
        </p>
      )}
    </Section>
  )
}

function useAccountChange() {
  const qc = useQueryClient()
  const toast = useToast()
  return useMutation({
    mutationFn: ({ id, change }: { id: number; change: Parameters<typeof api.setAccount>[1] }) => api.setAccount(id, change),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['accounts'] }),
    onError: (e) => toast.error("Couldn't change who it's shared with", errText(e)),
  })
}

function MyStoreAccount({ account: a, others }: { account: StoreAccount; others: Player[] }) {
  const change = useAccountChange()
  const part = (p: Player) => a.players.find((x) => x.player_id === p.id)
  // Who it's offered to now: everyone, or the ones ticked.
  const offeredTo = (p: Player) => a.sharing === 'everyone' || (a.sharing === 'chosen' && part(p)?.offered === true)
  return (
    <Row
      name={a.name}
      tag={`${storeName(a.store)} · ${gamesText(a.games)}`}
    >
      {/* Under the name rather than beside it, so it fits a small phone. */}
      <Segmented
        label={`Who ${a.name} is offered to`}
        options={SHARING}
        value={a.sharing}
        disabled={change.isPending}
        onChange={(sharing) => change.mutate({ id: a.id, change: { sharing } })}
      />
      {a.sharing !== 'owner' && others.length > 0 && (
        <div className="mt-2 flex flex-col gap-0.5">
          {others.map((p) => (
            <div key={p.id} className="flex flex-wrap items-center gap-x-5">
              {a.sharing === 'chosen' && (
                <Checkbox
                  checked={part(p)?.offered === true}
                  disabled={change.isPending}
                  onChange={(v) => change.mutate({ id: a.id, change: { players: [{ player_id: p.id, offered: v }] } })}
                >
                  Offer to {p.name}
                </Checkbox>
              )}
              {offeredTo(p) && (
                <Checkbox
                  checked={part(p)?.plays_on === true}
                  disabled={change.isPending}
                  onChange={(v) => change.mutate({ id: a.id, change: { players: [{ player_id: p.id, plays_on: v }] } })}
                  note={part(p)?.uses === false ? `${p.name} doesn’t use it` : 'its hours count for them too'}
                >
                  {p.name} also plays on it
                </Checkbox>
              )}
            </div>
          ))}
        </div>
      )}
    </Row>
  )
}

export function OfferedStoreAccount({ account: a }: { account: StoreAccount }) {
  const qc = useQueryClient()
  const toast = useToast()
  const use = useMutation({
    mutationFn: (uses: boolean) => api.useAccount(a.id, uses),
    // Its games come into the library or leave it.
    onSuccess: () => void qc.invalidateQueries(),
    onError: (e) => toast.error("Couldn't change it", errText(e)),
  })
  const name = `${possessive(a.owner)} ${a.name}`
  return (
    <Row name={name} tag={`${storeName(a.store)} · ${gamesText(a.games)}`} wrap desc={a.plays_on ? 'Its hours count for you too.' : 'Its games, without its hours.'}>
      <Checkbox checked={a.uses} disabled={use.isPending} onChange={(v) => use.mutate(v)}>
        Use these games
      </Checkbox>
    </Row>
  )
}

function MyAiAccount({ account: a, others }: { account: AiAccount; others: Player[] }) {
  const qc = useQueryClient()
  const toast = useToast()
  const { bound } = usePlayers()
  const change = useMutation({
    mutationFn: (c: Parameters<typeof api.setAiAccount>[1]) => api.setAiAccount(a.id, c),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['ai-accounts'] }),
    onError: (e) => toast.error("Couldn't change it", errText(e)),
  })
  const use = useUseAi(a)
  return (
    <Row
      name={aiName(a)}
      tag={[a.in_use && 'in use', a.library && 'the library’s AI'].filter(Boolean).join(' · ') || undefined}
    >
      <Segmented
        label={`Who ${aiName(a)} is offered to`}
        options={SHARING}
        value={a.sharing}
        disabled={change.isPending}
        onChange={(sharing) => change.mutate({ sharing, ...(sharing === 'chosen' ? { offered_to: a.offered_to } : {}) })}
      />
      <div className="mt-2 flex flex-col gap-0.5">
        {a.sharing === 'chosen' &&
          others.map((p) => (
            <Checkbox
              key={p.id}
              checked={a.offered_to.includes(p.id)}
              disabled={change.isPending}
              onChange={(v) => change.mutate({ offered_to: v ? [...a.offered_to, p.id] : a.offered_to.filter((id) => id !== p.id) })}
            >
              Offer to {p.name}
            </Checkbox>
          ))}
        <div className="flex flex-wrap items-center gap-x-4">
          {!a.in_use && (
            <TextButton disabled={use.isPending} onClick={() => use.mutate()}>
              Use it for your picks
            </TextButton>
          )}
          {!a.library && !bound && (
            <TextButton disabled={change.isPending} onClick={() => change.mutate({ library: true })}>
              Run the library’s summaries on it
            </TextButton>
          )}
        </div>
        {a.sharing !== 'owner' && (
          <p className="m-0 text-[13px] text-dim">Its keys are used for the others, never shown to them. Settings › AI usage shows what they cost you.</p>
        )}
      </div>
    </Row>
  )
}

function useUseAi(a: AiAccount) {
  const qc = useQueryClient()
  const toast = useToast()
  return useMutation({
    mutationFn: () => api.useAiAccount(a.id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['ai-accounts'] })
      void qc.invalidateQueries({ queryKey: keys.settings })
      void qc.invalidateQueries({ queryKey: keys.config })
      toast.success(`Your picks use ${aiName(a)}`)
    },
    onError: (e) => toast.error("Couldn't change your AI", errText(e)),
  })
}

export function OfferedAiAccount({ account: a }: { account: AiAccount }) {
  const use = useUseAi(a)
  return (
    <Row
      name={`${possessive(a.owner)} AI`}
      tag={aiName(a)}
      right={
        a.in_use ? (
          <Status tone="good">In use</Status>
        ) : (
          <TextButton disabled={use.isPending} onClick={() => use.mutate()}>
            Use it for your picks
          </TextButton>
        )
      }
    />
  )
}
