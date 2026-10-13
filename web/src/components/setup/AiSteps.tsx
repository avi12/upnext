import type { ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { Amount, FeatureEstimate, ScheduleChange, ScheduledResponse, SetupInfo, TaskId } from '../../types'
import { BACKEND_KEY, BACKEND_LABEL } from '../../lib/ai'
import { CodeText } from '../CodeText'
import { tokensText } from '../../lib/jobs'
import { AiChooser } from '../settings/Picks'
import { Row, Segmented } from '../settings/parts'
import { Skeleton } from '../Skeleton'
import { useToast } from '../useToast'
import { claudeInstall, flatpakClaude } from '../../lib/setup'
import { CopyLines, StepButtons, StepHead, type StepProps } from './parts'

const SUBSCRIPTION = ['claude-cli', 'codex-cli']

/** The wizard's "in parts": games a day for a big first run. A daily
 *  schedule runs every 24 hours from when it's set, so it says "a day",
 *  not "a night". */
const PART = 300

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** 6. Which AI: a subscription through its CLI, an API key, or another provider. */
export function AiStep({ setup, s, save, next, back }: StepProps) {
  const env = setup.environment
  const ai = setup.ai
  const estimates = useQuery({ queryKey: ['ai-estimates', s.llm_backend, s.digest_backend], queryFn: api.aiEstimates, staleTime: 60_000 })
  const pick = estimates.data?.features.find((f) => f.kind === 'pick')
  const perPick = pick && pick.monthly.items > 0 && pick.monthly.cost_usd != null ? pick.monthly.cost_usd / pick.monthly.items : null
  const install = claudeInstall(env.os)
  // In a container: the token from `claude setup-token`, since no browser can sign the CLI in there.
  const setupToken = (
    <>
      run <span className="font-mono text-[13px]">claude setup-token</span> on any computer where you use Claude, and give the
      container the token it prints as <span className="font-mono text-[13px]">CLAUDE_CODE_OAUTH_TOKEN</span> (compose: under{' '}
      <span className="font-mono text-[13px]">environment:</span>)
    </>
  )
  const where = env.container
    ? 'the container'
    : env.flatpak
      ? 'UpNext’s Flatpak'
      : env.client_local
        ? 'this computer'
        : 'the computer UpNext runs on'
  const subscription = (
    <li>
      <b className="font-semibold text-ink">Your Claude subscription</b>, through Claude Code {env.flatpak ? 'in' : 'on'} {where}.{' '}
      {env.container ? (
        <ContainerClaude ai={ai} setupToken={setupToken} />
      ) : env.flatpak ? (
        <FlatpakClaude ai={ai} id={env.flatpak} />
      ) : ai.claude ? (
        <>
          Found at <span className="font-mono text-[13px] break-words">{ai.claude}</span>; sign in once with{' '}
          <span className="font-mono text-[13px]">claude</span> in {install.shell} if you haven't.
        </>
      ) : (
        <>
          Not installed. In {install.shell}: <span className="font-mono text-[13px] break-words">{install.command}</span>, then run{' '}
          <span className="font-mono text-[13px]">claude</span> once to sign in.
        </>
      )}
    </li>
  )
  const apiKey = (
    <li>
      <b className="font-semibold text-ink">An API key</b> from Anthropic or OpenAI: paid per call
      {perPick != null && s.llm_backend === 'api-key' ? `, about $${perPick.toFixed(3)} a pick at list prices` : ''}.
    </li>
  )
  const other = (
    <li>
      <b className="font-semibold text-ink">Another provider</b>: any server that speaks OpenAI's API (OpenAI-compatible), your own
      (vLLM, llama.cpp, Ollama, LM Studio
      {env.container ? '; Ollama on the host is at http://host.docker.internal:11434/v1' : ''}) or a hosted one such as
      OpenRouter or Gemini. Choose “Your own server” under Provider to type its address.
    </li>
  )
  return (
    <section aria-labelledby="step-ai" className="flex flex-col gap-6">
      <StepHead id="step-ai" title="AI">
        <p className="m-0">
          The AI reads your library and picks what to play, with reasons, and writes the short summaries of what players say.
          UpNext works with:
        </p>
        <ul className="m-0 flex list-disc flex-col gap-1.5 pl-5">
          {env.container || env.flatpak ? (
            <>
              {apiKey}
              {other}
              {subscription}
            </>
          ) : (
            <>
              {subscription}
              {apiKey}
              {other}
            </>
          )}
        </ul>
        <p className="m-0">
          Choose one, then Test. Test makes one real AI call, which uses a few hundred tokens from your plan or API key; the
          result shows the exact count. Skipping is fine: the library works without AI, and picks still come, ranked by UpNext
          alone, but they won't fit you as well.
        </p>
      </StepHead>
      <AiChooser s={s} save={save} />
      {!ai.available && ai.error && <AiNotReady backend={ai.backend} error={ai.error} />}
      <StepButtons onNext={next} onBack={back} onSkip={next} />
    </section>
  )
}

/** Why the chosen AI can't run yet, each way out on its own line. A missing
 *  key is said in the wizard's words, since the key field is on this
 *  screen; anything else is the server's reason, its commands shown as code. */
export function AiNotReady({ backend, error }: { backend: string; error: string }) {
  const key = BACKEND_KEY[backend]
  const name = BACKEND_LABEL[backend] ?? backend
  if (key) {
    return (
      <div className="flex flex-col gap-1 text-[14px] text-warn" role="status">
        <p className="m-0">No {name} is saved yet: paste it in the Key field above.</p>
        <ul className="m-0 flex list-disc flex-col gap-0.5 pl-5 text-muted">
          <li>
            Or from a terminal: <code className="font-mono text-[0.85em] text-ink">upnext config set {key.setting}</code>
          </li>
          <li>
            Or set <code className="font-mono text-[0.85em] text-ink">{key.env}</code> where UpNext runs (its .env file, or the
            compose file in Docker)
          </li>
        </ul>
      </div>
    )
  }
  return (
    <div className="flex flex-col gap-1 text-[14px]" role="status">
      <p className="m-0 text-warn">{name} can't run yet.</p>
      <p className="m-0 text-muted">
        <CodeText text={error[0].toUpperCase() + error.slice(1).replace(/\.?$/, '.')} />
      </p>
    </div>
  )
}

/** The compose lines that install Claude Code in the container and sign it in. */
const CLAUDE_LINES = 'UPNEXT_INSTALL_CLAUDE: "1"\nCLAUDE_CODE_OAUTH_TOKEN: "the token claude setup-token prints"'

/**
 * Claude Code in UpNext's container: the image doesn't carry it,
 * since its licence doesn't allow passing it on, so the container installs it
 * from Anthropic into /data when asked, and signs it in with a token.
 */
export function ContainerClaude({ ai, setupToken }: { ai: SetupInfo['ai']; setupToken: ReactNode }) {
  const mono = 'font-mono text-[13px]'
  if (ai.claude) {
    return ai.claude_token ? (
      <>
        Installed at <span className={`${mono} break-all`}>{ai.claude}</span>, with a token in{' '}
        <span className={mono}>CLAUDE_CODE_OAUTH_TOKEN</span>. Choose it below and Test.
      </>
    ) : (
      <>
        Installed at <span className={`${mono} break-all`}>{ai.claude}</span>. To sign it in, {setupToken}, then{' '}
        <span className={mono}>docker compose up -d</span>.
      </>
    )
  }
  if (ai.claude_install?.installing) {
    return <>Installing Claude Code from Anthropic into the container now (about 250 MB); this step updates when it's done.</>
  }
  if (ai.claude_install?.error) {
    return (
      <>
        Installing Claude Code from Anthropic failed: {ai.claude_install.error}.{' '}
        <span className={mono}>docker exec upnext upnext install-claude</span> tries again.
      </>
    )
  }
  return (
    <span className="flex flex-col gap-2">
      <span>
        Claude Code isn’t in UpNext’s image, since its licence doesn’t allow passing it on, so the two above work without it. To
        use your subscription, the container installs it from Anthropic into <span className={mono}>/data</span> when it starts
        with these lines under <span className={mono}>environment:</span> ({setupToken}), then{' '}
        <span className={mono}>docker compose up -d</span>:
      </span>
      <CopyLines label="For the compose file" text={CLAUDE_LINES} />
    </span>
  )
}

/**
 * Claude Code in UpNext's Flatpak: the sandbox can't run
 * one installed on the computer, so `upnext install-claude` puts it in the
 * app's own folder, and a token from `claude setup-token` signs it in.
 */
export function FlatpakClaude({ ai, id }: { ai: SetupInfo['ai']; id: string }) {
  const mono = 'font-mono text-[13px]'
  const lines = flatpakClaude(id)
  const token = (
    <>
      run <span className={mono}>claude setup-token</span> on any computer where you use Claude, and give UpNext the token it
      prints from a terminal here, then start UpNext again
    </>
  )
  if (ai.claude) {
    return ai.claude_token ? (
      <>
        Installed at <span className={`${mono} break-all`}>{ai.claude}</span>, with a token in{' '}
        <span className={mono}>CLAUDE_CODE_OAUTH_TOKEN</span>. Choose it below and Test.
      </>
    ) : (
      <span className="flex flex-col gap-2">
        <span>
          Installed at <span className={`${mono} break-all`}>{ai.claude}</span>. To sign it in, {token}:
        </span>
        <CopyLines label="For a terminal" text={lines.token} />
      </span>
    )
  }
  return (
    <span className="flex flex-col gap-2">
      <span>
        Claude Code isn’t part of the Flatpak, since its licence doesn’t allow passing it on, and the sandbox can’t run one
        installed on the computer. To use your subscription, install it into UpNext’s own folder from a terminal; then, to sign
        it in, {token}:
      </span>
      <CopyLines label="For a terminal" text={`${lines.install}\n${lines.token}`} />
    </span>
  )
}

interface Feature {
  task: TaskId
  kind: FeatureEstimate['kind']
  name: string
  what: string
  /** The choices, as [value, label, change]. */
  choices: [string, string, ScheduleChange][]
}

const OFF: ScheduleChange = { schedule: 'off', limit: 0 }

const FEATURES: Feature[] = [
  {
    task: 'digests',
    kind: 'review_summaries',
    name: 'People say, Vibes and review tags',
    what: `Once for the games you have, then for new games after each details run. On does them all after the next details run; ${PART} a day writes ${PART} a day until it has caught up, so a big library doesn't use up your plan or rate limit at once.`,
    choices: [
      ['off', 'Off', OFF],
      ['on', 'On', { schedule: 'after:details', limit: 0 }],
      ['parts', `${PART} a day`, { schedule: 'daily', limit: PART }],
    ],
  },
  {
    task: 'series_guides',
    kind: 'series_guides',
    name: 'Series guides',
    what: 'Where to start each series and in what order; once per series, then for new ones.',
    choices: [
      ['off', 'Off', OFF],
      ['on', 'On', { schedule: 'after:details', limit: 0 }],
    ],
  },
  {
    task: 'second_chances',
    kind: 'second_chances',
    name: 'Second chances',
    what: 'Monthly: whether games you put down have changed enough to try again.',
    choices: [
      ['off', 'Off', OFF],
      ['on', 'Monthly', { schedule: 'monthly', limit: 0 }],
    ],
  },
  {
    task: 'hidden_gems',
    kind: 'hidden_gems',
    name: 'Hidden gems',
    what: 'After each sync: rates your unplayed, well-reviewed games for what makes them unusual.',
    choices: [
      ['off', 'Off', OFF],
      ['on', 'On', { schedule: 'after:sync', limit: 0 }],
    ],
  },
]

function amountText(a: Amount | null | undefined, priced: boolean): string | null {
  if (!a || a.calls === 0) return null
  const parts = [`${tokensText(a.input_tokens + a.output_tokens)} tokens`]
  if (priced && a.cost_usd != null) parts.push(`$${a.cost_usd < 1 ? a.cost_usd.toFixed(2) : a.cost_usd.toFixed(0)}`)
  if (a.minutes != null && a.minutes >= 1) parts.push(`about ${Math.round(a.minutes)} min`)
  return parts.join(', ')
}

/** 7. What the AI does: every AI feature with its estimate for this library,
 *  each choice setting that job's schedule (every AI job is off until here). */
export function AiJobsStep({ s, next, back }: StepProps) {
  const qc = useQueryClient()
  const toast = useToast()
  const estimates = useQuery({ queryKey: ['ai-estimates', s.llm_backend, s.digest_backend], queryFn: api.aiEstimates, staleTime: 60_000 })
  const scheduled = useQuery({ queryKey: ['scheduled'], queryFn: api.scheduled })
  const change = useMutation({
    mutationFn: ({ task, change }: { task: TaskId; change: ScheduleChange }) => api.setSchedule(task, change),
    onSuccess: (data) => qc.setQueryData<ScheduledResponse>(['scheduled'], data),
    onError: (e) => toast.error("Couldn't change that", errText(e)),
  })
  const subscription = SUBSCRIPTION.includes(s.llm_backend)
  const priced = estimates.data?.priced ?? false
  const pick = estimates.data?.features.find((f) => f.kind === 'pick')
  const rows = scheduled.data?.tasks ?? []

  const current = (f: Feature) => {
    const row = rows.find((r) => r.task === f.task)
    if (!row || row.schedule === 'off') return 'off'
    if (row.limit && f.choices.some(([v]) => v === 'parts')) return 'parts'
    return 'on'
  }

  return (
    <section aria-labelledby="step-ai-jobs" className="flex flex-col gap-6">
      <StepHead id="step-ai-jobs" title="What the AI does">
        <p className="m-0">
          Picks and Help me decide run only when you ask
          {pick
            ? `, about ${tokensText(pick.rate.input_tokens + pick.rate.output_tokens)} tokens a pick${priced && pick.monthly.cost_usd != null && pick.monthly.items ? ` ($${(pick.monthly.cost_usd / pick.monthly.items).toFixed(3)})` : ''}`
            : ''}
          . Below is what the AI runs on its own. Each is off until you turn it on here or in Settings › Jobs, where you can change
          it later. The numbers are for this library with the AI you chose
          {estimates.data?.features.some((f) => f.rate.basis === 'built_in') ? ', from UpNext’s own measurements until it has yours' : ''}.
        </p>
        {subscription && (
          <p className="m-0 text-ink">
            UpNext doesn't know your plan's limits, so it can't promise how much of your plan picks or review summaries will use. A
            big first run can use them up: {PART} a day spreads it out.
          </p>
        )}
      </StepHead>
      {estimates.isLoading || scheduled.isLoading ? (
        <div className="space-y-3" aria-busy="true">
          <Skeleton className="h-12 w-full" />
          <Skeleton className="h-12 w-full" />
        </div>
      ) : (
        <div className="max-w-[880px]">
          {FEATURES.map((f) => {
            const est = estimates.data?.features.find((e) => e.kind === f.kind)
            const once = amountText(est?.one_time, priced)
            const month = amountText(est?.monthly, priced)
            return (
              <Row
                key={f.task}
                name={f.name}
                wrap
                desc={
                  <>
                    {f.what}
                    {/* Summaries on an AI of their own. */}
                    {est && est.backend !== s.llm_backend && <> The summaries' AI: {BACKEND_LABEL[est.backend] ?? est.backend}.</>}
                    {once && <> First run: {once}.</>}
                    {month && <> A month: {month}.</>}
                  </>
                }
                right={
                  <Segmented
                    label={f.name}
                    value={current(f)}
                    options={f.choices.map(([value, label]) => ({ value, label }))}
                    onChange={(v) => {
                      const choice = f.choices.find(([c]) => c === v)
                      if (choice) change.mutate({ task: f.task, change: choice[2] })
                    }}
                  />
                }
              />
            )
          })}
        </div>
      )}
      <StepButtons onNext={next} onBack={back} />
    </section>
  )
}
