import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import { ToastProvider } from '../Toast'
import type { AiEstimates, ScheduledResponse, Settings, SetupInfo } from '../../types'
import { AiJobsStep, AiNotReady, ContainerClaude, FlatpakClaude } from './AiSteps'

function ai(change: Partial<SetupInfo['ai']>): SetupInfo['ai'] {
  return { backend: 'api-key', available: true, error: null, claude: null, claude_install: null, claude_token: false, ...change }
}

function text(a: SetupInfo['ai']) {
  const { container } = render(<ContainerClaude ai={a} setupToken="run claude setup-token" />)
  return container.textContent ?? ''
}

describe('Claude Code in the container', () => {
  it('gives the compose lines that install it, when nothing is installed', () => {
    expect(text(ai({}))).toMatch(/isn’t in UpNext’s image.*installs it from Anthropic into \/data/)
    const lines = screen.getByText(/UPNEXT_INSTALL_CLAUDE/)
    expect(lines.textContent).toBe('UPNEXT_INSTALL_CLAUDE: "1"\nCLAUDE_CODE_OAUTH_TOKEN: "the token claude setup-token prints"')
  })

  it('says it is installing, or why it failed and how to try again', () => {
    expect(text(ai({ claude_install: { installing: true, error: null } }))).toMatch(/^Installing Claude Code from Anthropic/)
    expect(text(ai({ claude_install: { installing: false, error: 'reaching it' } }))).toMatch(
      /failed: reaching it\. docker exec upnext upnext install-claude tries again/,
    )
  })

  it('once installed, asks for the token until the container has one', () => {
    const installed = ai({ claude: '/data/.local/bin/claude' })
    expect(text(installed)).toMatch(/Installed at \/data\/\.local\/bin\/claude\. To sign it in, run claude setup-token/)
    expect(text({ ...installed, claude_token: true })).toMatch(/with a token in CLAUDE_CODE_OAUTH_TOKEN\. Choose it below and Test/)
  })
})

describe('Claude Code in the Flatpak', () => {
  const id = 'io.github.zytocc.UpNext'
  const flatpak = (a: SetupInfo['ai']) => render(<FlatpakClaude ai={a} id={id} />).container.textContent ?? ''

  it('gives the commands that install it in the app’s folder and sign it in', () => {
    expect(flatpak(ai({}))).toMatch(/isn’t part of the Flatpak.*install it into UpNext’s own folder/)
    expect(screen.getByText(/install-claude/).textContent).toBe(
      `flatpak run --command=upnext ${id} install-claude\nflatpak override --user --env=CLAUDE_CODE_OAUTH_TOKEN=<the token> ${id}`,
    )
  })

  it('once installed, asks for the token until UpNext has one', () => {
    const installed = ai({ claude: `/home/you/.var/app/${id}/.local/bin/claude` })
    expect(flatpak(installed)).toMatch(/Installed at .*\.local\/bin\/claude\. To sign it in, run claude setup-token/)
    expect(flatpak({ ...installed, claude_token: true })).toMatch(/with a token in CLAUDE_CODE_OAUTH_TOKEN\. Choose it below and Test/)
  })
})

describe('The AI step, when the AI can’t run', () => {
  it('asks for a missing key in its own words, each way on its own line', () => {
    const { container } = render(
      <AiNotReady
        backend="api-key"
        error="no Anthropic API key: add one on the Settings page, with `upnext config set anthropic_api_key`, or set ANTHROPIC_API_KEY"
      />,
    )
    expect(screen.getByText('No Anthropic API key is saved yet: paste it in the Key field above.')).toBeTruthy()
    const lines = [...container.querySelectorAll('li')].map((li) => li.textContent)
    expect(lines).toEqual([
      'Or from a terminal: upnext config set anthropic_api_key',
      'Or set ANTHROPIC_API_KEY where UpNext runs (its .env file, or the compose file in Docker)',
    ])
    expect(container.textContent).not.toMatch(/Settings page|`/)
  })

  it('names the OpenAI key’s setting for that backend', () => {
    render(<AiNotReady backend="openai-key" error="no OpenAI API key" />)
    expect(screen.getByText('upnext config set openai_key')).toBeTruthy()
    expect(screen.getByText('OPENAI_API_KEY')).toBeTruthy()
  })

  it('gives any other reason on its own line, with its commands as code', () => {
    const { container } = render(<AiNotReady backend="codex-cli" error="Codex isn't signed in: run `codex login`" />)
    expect(screen.getByText("ChatGPT subscription can't run yet.")).toBeTruthy()
    expect(container.querySelector('code')?.textContent).toBe('codex login')
  })
})

describe('What the AI does', () => {
  afterEach(() => vi.restoreAllMocks())

  it('says picks run when you ask in the intro, not as a row, and offers 300 a day with why', async () => {
    const amount = { items: 10, calls: 10, input_tokens: 25_000, output_tokens: 5_000, cost_usd: 1.5, basis: 'built_in' }
    vi.spyOn(api, 'aiEstimates').mockResolvedValue({
      backend: 'api-key',
      priced: true,
      features: [{ kind: 'pick', rate: { input_tokens: 25_000, output_tokens: 5_000, items_per_call: 1, basis: 'built_in', calls: 0 }, monthly: amount }],
    } as AiEstimates)
    vi.spyOn(api, 'scheduled').mockResolvedValue({ enabled: true, waiting_for_first_sync: true, tasks: [] } as ScheduledResponse)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const props = { setup: {} as SetupInfo, s: { llm_backend: 'api-key' } as Settings, save: vi.fn(), go: vi.fn(), next: vi.fn(), back: vi.fn() }
    const { container } = render(
      <QueryClientProvider client={client}>
        <ToastProvider>
          <AiJobsStep {...props} />
        </ToastProvider>
      </QueryClientProvider>,
    )
    expect(await screen.findByRole('radio', { name: '300 a day' })).toBeTruthy()
    expect(container.textContent).toMatch(/Picks and Help me decide run only when you ask, about 30(\.0)?k tokens a pick \(\$0\.150\)\. Below/)
    expect(screen.queryByText('Always')).toBeNull()
    expect(container.textContent).toMatch(/300 a day writes 300 a day until it has caught up/)
    expect(container.textContent).not.toMatch(/a night/)
  })
})
