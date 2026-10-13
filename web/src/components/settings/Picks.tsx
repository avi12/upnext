import { useRef, useState, type ReactNode } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { CodexModel, LlmTestResult, Settings } from '../../types'
import { durationText, plural } from '../../lib/format'
import { usageText } from '../../lib/jobs'
import { BACKEND_LABEL, summaryBackendOf } from '../../lib/ai'
import { CodeText } from '../CodeText'
import { Money } from '../Money'
import { InlineField, Row, RowGrid, Section, Segmented, TextButton } from './parts'
import type { SaveFn } from './useSettingsSave'
import { useToast } from '../useToast'
import { everyones, possessive, usePlayers } from '../../lib/players'

/** Services that speak the OpenAI chat API: picking one fills in the address. */
const PROVIDERS: { id: string; label: string; url: string; key: boolean }[] = [
  { id: 'ollama', label: 'Ollama (this computer)', url: 'http://localhost:11434/v1', key: false },
  { id: 'lmstudio', label: 'LM Studio (this computer)', url: 'http://localhost:1234/v1', key: false },
  { id: 'gemini', label: 'Google Gemini', url: 'https://generativelanguage.googleapis.com/v1beta/openai', key: true },
  { id: 'openrouter', label: 'OpenRouter', url: 'https://openrouter.ai/api/v1', key: true },
  { id: 'groq', label: 'Groq', url: 'https://api.groq.com/openai/v1', key: true },
  { id: 'mistral', label: 'Mistral', url: 'https://api.mistral.ai/v1', key: true },
  { id: 'deepseek', label: 'DeepSeek', url: 'https://api.deepseek.com/v1', key: true },
  { id: 'xai', label: 'xAI', url: 'https://api.x.ai/v1', key: true },
]

function providerOf(url: string | null | undefined) {
  const u = (url ?? '').replace(/\/+$/, '')
  return PROVIDERS.find((p) => p.url === u)?.id ?? 'custom'
}

function modelLabel(m: string) {
  return m === 'default' ? 'Default' : m[0].toUpperCase() + m.slice(1)
}

/** The AI backend and its fields, with Test: Settings › Picks, and the setup
 *  wizard's AI step. With `purpose` "summaries", the backend that writes
 *  review summaries, second chances and hidden gems: "Same as picks",
 *  or another with its own fields, of which only the summaries' model. */
export function AiChooser({ s, save, purpose = 'picks' }: { s: Settings; save: SaveFn; purpose?: 'picks' | 'summaries' }) {
  const toast = useToast()
  const forSummaries = purpose === 'summaries'
  const summaryBackend = summaryBackendOf(s)
  const apart = summaryBackend !== s.llm_backend
  // The summaries' chooser shows a backend's fields only when it isn't the
  // picks' one; the picks' fields leave out a summaries model that isn't used.
  const fields = !forSummaries || apart
  const showPicks = !forSummaries
  const showSummaries = forSummaries || !apart
  const [test, setTest] = useState<LlmTestResult | { ok: false; error: string } | null>(null)
  const llmTest = useMutation({
    mutationFn: () => api.llmTest(forSummaries ? 'summaries' : undefined),
    onMutate: () => setTest(null),
    onSuccess: (r) => {
      setTest(r)
      if (!r.ok) toast.error("The AI didn't answer", r.error ?? undefined)
    },
    onError: (e) => {
      const error = e instanceof ApiError ? e.message : String(e)
      setTest({ ok: false, error })
      toast.error("The AI didn't answer", error)
    },
  })
  const backend = forSummaries ? summaryBackend : s.llm_backend
  const local = fields && backend === 'openai'
  const codex = fields && backend === 'codex-cli'
  const openaiKey = fields && backend === 'openai-key'
  // Set in the fields for the backend, not in the model rows.
  const ownModels = local || codex || openaiKey
  // "Your own server" chosen, before its address is typed: the saved
  // address still names a preset until then.
  const [ownServer, setOwnServer] = useState(false)
  const addressRef = useRef<HTMLInputElement>(null)
  const provider = ownServer ? 'custom' : providerOf(s.openai_base_url)
  const hosted = PROVIDERS.find((p) => p.id === provider)?.key ?? true
  // Suggestions for the model fields, straight from the server.
  const models = useQuery({
    queryKey: ['llm-models', s.openai_base_url, s.openai_api_key_set],
    queryFn: api.llmModels,
    enabled: local,
    staleTime: 300_000,
  })
  const modelList = models.data?.models ?? []
  // OpenAI has a choice of its own, which holds replies to their format.
  const openaiAddress = /^https:\/\/api\.openai\.com(\/|$)/.test(s.openai_base_url ?? '')
  const backends = s.llm_backends.length ? s.llm_backends : Object.keys(BACKEND_LABEL)

  const testControl = (
    <div className="flex min-h-11 min-w-0 items-center gap-3">
      <TextButton
        disabled={llmTest.isPending}
        title="Makes one real AI call, a few hundred tokens from your plan or API key"
        onClick={() => llmTest.mutate()}
      >
        {llmTest.isPending ? 'Testing…' : 'Test'}
      </TextButton>
      <span aria-live="polite" className="min-w-0 text-sm">
        {test?.ok && (
          <span className="text-good">
            Works{'model' in test && test.model ? ` · ${test.model}` : ''}
            {'duration_ms' in test && test.duration_ms != null ? ` · ${durationText(test.duration_ms)}` : ''}
            {'usage' in test && test.usage ? ` · ${usageText(test.usage)}` : ''}
            {'usage' in test && <Money cost={test} link prefix=" · " />}
          </span>
        )}
        {test && !test.ok && (
          <span className="break-words text-bad">
            <CodeText text={test.error ?? "Didn't work"} />
          </span>
        )}
      </span>
    </div>
  )

  return (
    <div className="flex flex-wrap items-center gap-x-5 gap-y-3 border-b border-line py-3.5">
      <span className="w-[120px] shrink-0 text-[15.5px] font-bold">{forSummaries ? 'Summaries' : 'AI'}</span>
      {forSummaries ? (
        <Segmented
          label="AI for summaries"
          className="max-w-full flex-wrap"
          value={apart ? summaryBackend : 'same'}
          options={['same', ...backends.filter((b) => b !== s.llm_backend)].map((b) => ({
            value: b,
            label: b === 'same' ? 'Same as picks' : (BACKEND_LABEL[b] ?? b),
          }))}
          onChange={(b) => {
            setTest(null)
            void save({ digest_backend: b }, b === 'same' ? 'Summaries: the same AI as picks' : `Summaries: ${BACKEND_LABEL[b] ?? b}`)
          }}
        />
      ) : (
        <Segmented
          label="AI backend"
          className="max-w-full flex-wrap"
          value={backend}
          options={backends.map((b) => ({ value: b, label: BACKEND_LABEL[b] ?? b }))}
          onChange={(b) => {
            setTest(null)
            void save({ llm_backend: b }, `AI: ${BACKEND_LABEL[b] ?? b}`)
          }}
        />
      )}
      {forSummaries && (
        <p className="m-0 basis-full text-[13px] text-dim lg:pl-[140px]">
          Writes the "People say" lines, review tags, second chances and hidden gems. A model at home or a cheaper key can
          do this bulk work while picks stay on the AI above.
        </p>
      )}
      {fields && backend === 'api-key' && (
        <InlineField
          key="anthropic"
          label="Key"
          secret
          value=""
          placeholder={
            s.anthropic_api_key_set
              ? `Saved${s.anthropic_api_key_source === 'env' ? ' in .env' : ''}. Paste to replace`
              : 'sk-ant-…'
          }
          className="w-[260px] font-mono text-xs"
          onCommit={(v) => save({ anthropic_api_key: v }, 'Anthropic API key', { inline: true })}
        />
      )}

      {local && (
        <div className="flex min-w-0 basis-full flex-wrap items-start gap-x-5 gap-y-3 lg:pl-[140px]">
          <label className="flex min-h-11 max-w-full min-w-0 items-center gap-2 text-[13.5px] text-dim">
            Provider
            <select
              className="field w-[210px] min-w-0 text-sm"
              value={provider}
              onChange={(e) => {
                const p = PROVIDERS.find((x) => x.id === e.target.value)
                setOwnServer(!p)
                if (p) void save({ openai_base_url: p.url }, `AI provider: ${p.label}`)
                else requestAnimationFrame(() => addressRef.current?.focus())
              }}
            >
              <option value="custom">Your own server (vLLM, llama.cpp, any OpenAI-compatible)</option>
              {PROVIDERS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          </label>
          <InlineField
            key={ownServer ? 'own-server' : 'server'}
            ref={addressRef}
            label="Address"
            // Your own server starts empty, for its address to be typed.
            value={ownServer && providerOf(s.openai_base_url) !== 'custom' ? '' : (s.openai_base_url ?? '')}
            placeholder={provider === 'custom' ? 'http://192.168.0.42:8000/v1' : 'http://localhost:11434/v1'}
            inputMode="url"
            className="w-[260px] font-mono text-xs"
            onCommit={async (v) => {
              const err = await save({ openai_base_url: v }, 'AI server', { inline: true })
              if (!err) setOwnServer(false)
              return err
            }}
          />
          <InlineField
            key="okey"
            label="Key"
            secret
            value=""
            placeholder={s.openai_api_key_set ? 'Saved. Paste to replace' : hosted ? 'Needed for this provider' : 'Optional'}
            className="w-[190px] font-mono text-xs"
            onCommit={(v) => save({ openai_api_key: v }, 'AI provider key', { inline: true })}
          />
          {!s.listen.this_computer && s.openai_api_key_set && (
            <p className="m-0 basis-full text-[13.5px] text-dim">
              A new address set from this device clears the saved key, so it can't be sent somewhere new. Paste the key
              again after changing it.
            </p>
          )}
          <datalist id="llm-models">
            {modelList.map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
          {showPicks && (
            <InlineField
              key="model"
              label="Picks model"
              list="llm-models"
              value={s.openai_model ?? ''}
              placeholder={modelList.length ? `${modelList.length} available` : 'Model id'}
              className="w-[230px]"
              onCommit={(v) => save({ openai_model: v }, 'AI model', { inline: true })}
            />
          )}
          {showSummaries && (
            <InlineField
              key="dmodel"
              label="Summaries model"
              list="llm-models"
              value={s.openai_digest_model ?? ''}
              placeholder={
                !forSummaries
                  ? 'Same as picks (a smaller one saves money)'
                  : s.openai_model
                    ? `The server's model, ${s.openai_model}`
                    : 'Model id'
              }
              className="w-[270px]"
              onCommit={(v) => save({ openai_digest_model: v }, 'Summaries model', { inline: true })}
            />
          )}
          <label className="flex min-h-11 max-w-full min-w-0 items-center gap-2 text-[13.5px] text-dim">
            Thinking
            <select
              className="field w-[230px] min-w-0 text-sm"
              value={s.openai_thinking ?? 'auto'}
              onChange={(e) => void save({ openai_thinking: e.target.value }, `Thinking: ${THINKING_LABEL[e.target.value] ?? e.target.value}`)}
            >
              {Object.entries(THINKING_LABEL).map(([v, label]) => (
                <option key={v} value={v}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          {testControl}
          {models.data && !models.data.ok && !ownServer && (
            <p className="m-0 basis-full text-[13px] text-dim" title={models.data.error ?? undefined}>
              {models.data.error?.startsWith('cannot reach')
                ? `Nothing answering at ${s.openai_base_url} yet: start the server there, or choose another.`
                : `Couldn't list models: ${models.data.error}`}
            </p>
          )}
          {openaiAddress && (
            <p className="m-0 basis-full text-[13px] text-dim">
              For OpenAI, choose {BACKEND_LABEL['openai-key']} above instead: it holds every reply to the format UpNext asks
              for, uses the newest Luna, and runs summaries at half price.
            </p>
          )}
        </div>
      )}
      {codex && <CodexFields s={s} save={save} testControl={testControl} showPicks={showPicks} showSummaries={showSummaries} />}
      {openaiKey && (
        <OpenAiKeyFields s={s} save={save} testControl={testControl} showPicks={showPicks} showSummaries={showSummaries} />
      )}
      {fields && !ownModels && testControl}
    </div>
  )
}

/** `openai_thinking`: a model that reasons before it answers (Qwen) can take
 *  minutes a pick on a computer at home, so "auto" turns it off there. */
const THINKING_LABEL: Record<string, string> = {
  auto: 'Off on a home server',
  off: 'Off',
  on: "The model's default",
}

/** With two or more players: whose AI your picks use, when it's
 *  someone else's, since changing these settings then makes one of your own. */
function SharedAiNote() {
  const { multi } = usePlayers()
  const ai = useQuery({ queryKey: ['ai-accounts'], queryFn: api.aiAccounts, enabled: multi })
  const used = ai.data?.find((a) => a.in_use)
  if (!multi || !used || used.mine) return null
  return (
    <p className="m-0 border-b border-line py-3 text-[14px] text-muted">
      Your picks use {possessive(used.owner)} AI, shared with you. Changing the AI here sets up one of your own instead; Sharing lists the
      ones offered to you.
    </p>
  )
}

export function PicksSection({ s, save }: { s: Settings; save: SaveFn }) {
  const { multi } = usePlayers()
  // Set in the fields for the backend, not in the model rows.
  const ownModels = (b: string) => b === 'openai' || b === 'codex-cli' || b === 'openai-key'
  // Summaries may have a backend of their own.
  const summaryBackend = summaryBackendOf(s)
  // An Anthropic API key's own defaults: Sonnet 5.5 for picks, Haiku 5.5
  // for summaries.
  const labelAs = (backend: string, fallback: string) => (m: string) =>
    backend === 'api-key' && m === 'default' ? fallback : modelLabel(m)
  const pickLabel = labelAs(s.llm_backend, 'Sonnet 5.5')
  const summaryLabel = labelAs(summaryBackend, 'Haiku 5.5')
  return (
    <Section id="picks" title="Picks" desc="Which AI chooses your lineup and writes the summaries, and how.">
      <SharedAiNote />
      <AiChooser s={s} save={save} />
      <AiChooser s={s} save={save} purpose="summaries" />
      <RowGrid>
        <ModelRow
          name="Model for picks"
          desc={
            ownModels(s.llm_backend)
              ? 'Set above for this provider'
              : 'Sonnet is a good balance; Opus is sharper and costs about 2.5x more'
          }
          value={s.model}
          models={s.models}
          label={pickLabel}
          disabled={ownModels(s.llm_backend)}
          onChange={(m) => save({ model: m }, `Model for picks: ${pickLabel(m)}`)}
        />
        <ModelRow
          name="Model for review summaries"
          desc={
            ownModels(summaryBackend)
              ? 'Set above for this provider'
              : summaryBackend === 'api-key'
                ? 'Writes the "People say" lines and the review tags; Haiku costs about a twentieth of Sonnet'
                : 'Writes the "People say" lines and the review tags'
          }
          value={s.digest_model}
          models={s.digest_models ?? s.models}
          label={summaryLabel}
          disabled={ownModels(summaryBackend)}
          onChange={(m) => save({ digest_model: m }, `Model for review summaries: ${summaryLabel(m)}`)}
        />
        <Row
          name="When is a game played?"
          desc="Shorter than this counts as sampled"
          right={
            <InlineField
              label="After"
              value={String(s.sampled_minutes)}
              inputMode="numeric"
              className="w-20 text-right tabular-nums"
              wrapClassName="items-end"
              after={<span className="text-sm text-muted">min</span>}
              onCommit={(v) => save({ sampled_minutes: v }, `Played after ${plural(Number(v), 'minute')}`, { inline: true })}
            />
          }
        />
        <Row
          name="When is a game shelved?"
          desc="Started, unfinished and untouched this long"
          right={
            <InlineField
              label="After"
              value={String(s.shelve_after_days)}
              inputMode="numeric"
              className="w-20 text-right tabular-nums"
              wrapClassName="items-end"
              after={<span className="text-sm text-muted">days</span>}
              onCommit={(v) => save({ shelve_after_days: v }, `Shelved after ${plural(Number(v), 'day')}`, { inline: true })}
            />
          }
        />
        <Row
          name="When is it abandoned?"
          desc="Shelved and untouched this long"
          right={
            <InlineField
              label="After"
              value={String(s.abandon_after_days)}
              inputMode="numeric"
              className="w-20 text-right tabular-nums"
              wrapClassName="items-end"
              after={<span className="text-sm text-muted">days</span>}
              onCommit={(v) => save({ abandon_after_days: v }, `Abandoned after ${plural(Number(v), 'day')}`, { inline: true })}
            />
          }
        />
        <Row
          name="Staples"
          desc="Games you've played this long are your staples, never shelved"
          right={
            <InlineField
              label="At"
              value={String(s.staple_hours)}
              inputMode="numeric"
              className="w-20 text-right tabular-nums"
              wrapClassName="items-end"
              after={<span className="text-sm text-muted">hours</span>}
              onCommit={(v) => save({ staple_hours: v }, `Staples at ${plural(Number(v), 'hour')}`, { inline: true })}
            />
          }
        />
        <Row
          name="Burnt out lasts"
          desc="Then the game comes back to picks by itself"
          right={
            <InlineField
              label="For"
              value={String(s.burnout_days)}
              inputMode="numeric"
              className="w-20 text-right tabular-nums"
              wrapClassName="items-end"
              after={<span className="text-sm text-muted">days</span>}
              onCommit={(v) => save({ burnout_days: v }, `Burnt out for ${plural(Number(v), 'day')}`, { inline: true })}
            />
          }
        />
        <Row
          name="Wishlist and suggestions"
          desc="Games you don't own from your series, and your Steam wishlist, in Lists"
          right={
            <Segmented
              label="Wishlist and suggestions"
              value={s.wishlist ? 'on' : 'off'}
              options={[
                { value: 'on', label: 'On' },
                { value: 'off', label: 'Off' },
              ]}
              onChange={(v) => void save({ wishlist: v === 'on' }, v === 'on' ? 'Wishlist and suggestions on' : 'Wishlist and suggestions off')}
            />
          }
        />
        <Row
          name="Games the AI looks at"
          tag={everyones(multi)}
          desc="More gives the AI wider choice but costs more per pick (default 120)"
          right={
            <InlineField
              label="Up to"
              value={String(s.max_candidates)}
              inputMode="numeric"
              className="w-20 text-right tabular-nums"
              wrapClassName="items-end"
              after={<span className="text-sm whitespace-nowrap text-muted">per pick</span>}
              onCommit={(v) => save({ max_candidates: v }, `Up to ${v} games per pick`, { inline: true })}
            />
          }
        />
      </RowGrid>
    </Section>
  )
}

/** Which of a backend's two model fields to show: both, or only the picks'
 *  or the summaries' one when summaries have a backend of their own. */
type FieldsFor = { showPicks: boolean; showSummaries: boolean }

/** The ChatGPT subscription through the Codex CLI: whether it's signed in,
 *  the two models from the account's own list, and the reasoning effort. */
function CodexFields({
  s,
  save,
  testControl,
  showPicks,
  showSummaries,
}: { s: Settings; save: SaveFn; testControl: ReactNode } & FieldsFor) {
  const status = useQuery({ queryKey: ['llm-codex'], queryFn: api.llmCodex, staleTime: 60_000 })
  const c = status.data
  const models: CodexModel[] = c?.models ?? []
  const families = c?.families ?? []
  // A family ("luna") stands for its newest model.
  const byId = (id: string | null) => models.find((m) => m.id === (families.find((f) => f.id === id)?.model ?? id))
  const modelLabel = (id: string) => {
    const f = families.find((x) => x.id === id)
    const m = byId(id)
    return f ? `Newest ${id[0].toUpperCase() + id.slice(1)}${m ? ` (${m.name})` : ''}` : (m?.name ?? id)
  }
  const pickModel = showPicks ? byId(s.codex_model) : undefined
  const summaryModel = showSummaries ? byId(s.codex_digest_model) : undefined
  // The efforts the chosen models shown take; all of them while the list is unknown.
  const efforts = s.codex_efforts.filter(
    (e) => (!pickModel || pickModel.efforts.includes(e)) && (!summaryModel || summaryModel.efforts.includes(e)),
  )
  if (s.codex_effort && !efforts.includes(s.codex_effort)) efforts.push(s.codex_effort)

  let state: ReactNode
  if (status.isPending) state = <span className="text-dim">Checking Codex…</span>
  else if (status.isError) state = <span className="text-bad">Couldn't check Codex: {String(status.error)}</span>
  else if (c && !c.installed)
    state = (
      <span className="text-bad">
        Codex isn't installed here. Install it with <code className="font-mono text-[0.85em]">npm i -g @openai/codex</code>, then sign in with <code className="font-mono text-[0.85em]">codex login</code>{' '}
        in a terminal.
      </span>
    )
  else if (c && !c.login?.signed_in)
    state = (
      <span className="text-bad">
        Codex isn't signed in. Run <code className="font-mono text-[0.85em]">codex login</code> in a terminal, then check again.
      </span>
    )
  else if (c?.login?.per_call)
    state = (
      <span className="text-bad">
        Codex is signed in with an API key, which bills each call to that key. Sign in with ChatGPT (<code className="font-mono text-[0.85em]">codex login</code>), or
        use Another provider for an OpenAI key.
      </span>
    )
  else if (c) state = <span className="text-good">Signed in with ChatGPT{c.version ? ` · ${c.version}` : ''}</span>

  const modelSelect = (label: string, value: string | null, onChange: (m: string) => void) => (
    <label className="flex min-h-11 max-w-full min-w-0 items-center gap-2 text-[13.5px] text-dim">
      {label}
      <select className="field w-[240px] min-w-0 text-sm" value={value ?? ''} onChange={(e) => onChange(e.target.value)}>
        {value && !byId(value) && <option value={value}>{value}</option>}
        {families.map((f) => (
          <option key={f.id} value={f.id} title="Moves to a newer model of the family as soon as Codex lists one">
            {modelLabel(f.id)}
          </option>
        ))}
        <optgroup label="Exactly this model">
          {models.map((m) => (
            <option key={m.id} value={m.id} title={m.description}>
              {m.name}
            </option>
          ))}
        </optgroup>
      </select>
    </label>
  )

  return (
    <div className="flex min-w-0 basis-full flex-wrap items-start gap-x-5 gap-y-3 lg:pl-[140px]">
      <p aria-live="polite" className="m-0 flex min-h-11 min-w-0 basis-full flex-wrap items-center gap-x-3 text-sm [overflow-wrap:anywhere]">
        {state}
        {!status.isPending && (
          <TextButton disabled={status.isFetching} onClick={() => void status.refetch()}>
            {status.isFetching ? 'Checking…' : 'Check again'}
          </TextButton>
        )}
      </p>
      {showPicks &&
        modelSelect('Picks model', s.codex_model, (m) => void save({ codex_model: m }, `Picks model: ${modelLabel(m)}`))}
      {showSummaries &&
        modelSelect(
          'Summaries model',
          s.codex_digest_model,
          (m) => void save({ codex_digest_model: m }, `Summaries model: ${modelLabel(m)}`),
        )}
      <label className="flex min-h-11 max-w-full min-w-0 items-center gap-2 text-[13.5px] text-dim">
        Reasoning
        <select
          className="field w-[150px] text-sm"
          value={s.codex_effort ?? ''}
          onChange={(e) => void save({ codex_effort: e.target.value || null }, `Reasoning: ${e.target.value || 'Codex default'}`)}
        >
          <option value="">Codex default</option>
          {efforts.map((e) => (
            <option key={e} value={e}>
              {e === 'xhigh' ? 'Extra high' : e[0].toUpperCase() + e.slice(1)}
            </option>
          ))}
        </select>
      </label>
      {testControl}
      <p className="m-0 basis-full text-[13px] text-dim">
        Every AI call uses part of your ChatGPT plan's limits, shared with Codex wherever you use it. Bigger models and more
        reasoning allow fewer calls per 5 hours.
        {c?.models_from === 'built_in' && ` Codex's own model list couldn't be read, so these are UpNext's: ${c.models_error}`}
      </p>
    </div>
  )
}

/** The OpenAI API with the user's key: the two models from the key's own
 *  list, the reasoning effort, and how the calls are billed. */
function OpenAiKeyFields({
  s,
  save,
  testControl,
  showPicks,
  showSummaries,
}: { s: Settings; save: SaveFn; testControl: ReactNode } & FieldsFor) {
  const status = useQuery({ queryKey: ['llm-openai', s.openai_key_set], queryFn: api.llmOpenAi, staleTime: 300_000 })
  const o = status.data
  const models = o?.models ?? []
  const families = o?.families ?? []
  // A family ("luna") stands for its newest model.
  const modelLabel = (id: string) => {
    const f = families.find((x) => x.id === id)
    return f ? `Newest ${id[0].toUpperCase() + id.slice(1)} (${f.model})` : id
  }

  let state: ReactNode
  if (!s.openai_key_set)
    state = (
      <span className="text-dim">
        Paste a key from <span className="font-mono text-[0.85em]">platform.openai.com/api-keys</span>. The account needs
        prepaid credit.
      </span>
    )
  else if (status.isPending) state = <span className="text-dim">Checking the key…</span>
  else if (status.isError) state = <span className="text-bad">Couldn't check the key: {String(status.error)}</span>
  else if (o?.models_from === 'built_in')
    state = (
      <span className="text-bad">
        <CodeText text={o.models_error ?? "Couldn't read the key's models"} />
      </span>
    )
  else if (o) state = <span className="text-good">OpenAI accepts the key · {models.length} GPT models</span>

  const modelSelect = (label: string, value: string | null, onChange: (m: string) => void) => (
    <label className="flex min-h-11 max-w-full min-w-0 items-center gap-2 text-[13.5px] text-dim">
      {label}
      <select className="field w-[240px] min-w-0 text-sm" value={value ?? ''} onChange={(e) => onChange(e.target.value)}>
        {value && !families.some((f) => f.id === value) && !models.includes(value) && <option value={value}>{value}</option>}
        {families.map((f) => (
          <option key={f.id} value={f.id} title="Moves to a newer model of the family as soon as OpenAI lists one">
            {modelLabel(f.id)}
          </option>
        ))}
        <optgroup label="Exactly this model">
          {models.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </optgroup>
      </select>
    </label>
  )

  return (
    <div className="flex min-w-0 basis-full flex-wrap items-start gap-x-5 gap-y-3 lg:pl-[140px]">
      <InlineField
        key="openai-key"
        label="Key"
        secret
        value=""
        placeholder={s.openai_key_set ? `Saved${s.openai_key_source === 'env' ? ' in .env' : ''}. Paste to replace` : 'sk-…'}
        className="w-[260px] font-mono text-xs"
        onCommit={(v) => save({ openai_key: v }, 'OpenAI API key', { inline: true })}
      />
      <p aria-live="polite" className="m-0 flex min-h-11 min-w-0 basis-full flex-wrap items-center gap-x-3 text-sm [overflow-wrap:anywhere]">
        {state}
        {s.openai_key_set && !status.isPending && (
          <TextButton disabled={status.isFetching} onClick={() => void status.refetch()}>
            {status.isFetching ? 'Checking…' : 'Check again'}
          </TextButton>
        )}
      </p>
      {showPicks &&
        modelSelect('Picks model', s.openai_key_model, (m) => void save({ openai_key_model: m }, `Picks model: ${modelLabel(m)}`))}
      {showSummaries &&
        modelSelect(
          'Summaries model',
          s.openai_key_digest_model,
          (m) => void save({ openai_key_digest_model: m }, `Summaries model: ${modelLabel(m)}`),
        )}
      <label className="flex min-h-11 max-w-full min-w-0 items-center gap-2 text-[13.5px] text-dim">
        Reasoning
        <select
          className="field w-[150px] text-sm"
          value={s.openai_key_effort ?? ''}
          onChange={(e) =>
            void save({ openai_key_effort: e.target.value || null }, `Reasoning: ${e.target.value || 'OpenAI default'}`)
          }
        >
          <option value="">OpenAI default</option>
          {s.openai_key_efforts.map((e) => (
            <option key={e} value={e}>
              {e === 'xhigh' ? 'Extra high' : e[0].toUpperCase() + e.slice(1)}
            </option>
          ))}
        </select>
      </label>
      {testControl}
      <p className="m-0 basis-full text-[13px] text-dim">
        Every call is billed to the key at OpenAI's prices. Review summaries and series guides wait for OpenAI's spare
        capacity (Flex), at half price; picks go straight away at the full price.
      </p>
    </div>
  )
}

function ModelRow({
  name,
  desc,
  value,
  models,
  label,
  disabled,
  onChange,
}: {
  name: string
  desc: string
  value: string
  models: string[]
  label: (m: string) => string
  disabled: boolean
  onChange: (m: string) => void
}) {
  return (
    <Row
      name={name}
      desc={desc}
      right={
        <select
          aria-label={name}
          className="field w-[130px] shrink-0 text-sm disabled:opacity-50"
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
        >
          {models.map((m) => (
            <option key={m} value={m}>
              {label(m)}
            </option>
          ))}
        </select>
      }
    />
  )
}
