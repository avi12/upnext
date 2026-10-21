import type { AiAccount, AiKind } from '../types'

/** What the Settings AI switch calls each backend. */
export const BACKEND_LABEL: Record<string, string> = {
  'claude-cli': 'Claude subscription',
  'codex-cli': 'ChatGPT subscription',
  'api-key': 'Anthropic API key',
  'openai-key': 'OpenAI API key',
  openai: 'Another provider',
  copilot: 'Microsoft Copilot',
}

/** The backend that writes review summaries, second chances and hidden
 *  gems: the summaries' own when Settings names one, else the picks'. */
export function summaryBackendOf(s: { llm_backend: string; digest_backend?: string | null }): string {
  return s.digest_backend && s.digest_backend !== 'same' ? s.digest_backend : s.llm_backend
}

/** What an AI call was for, as AI usage lists it. */
export const KIND_LABEL: Record<AiKind, string> = {
  pick: 'Picks',
  decide: 'Help me decide',
  too: "It's too ___",
  review_summaries: 'People say, Vibes and tags',
  series_guides: 'Series guides',
  second_chances: 'Second chances',
  hidden_gems: 'Hidden gems',
  test: 'Tests',
}

/** The key each API-key backend needs. Such a backend can't run only when
 *  its key is missing, so the UI says so in its own words rather than
 *  showing the server's message, which is written for the command line. */
export const BACKEND_KEY: Record<string, { setting: string; env: string }> = {
  'api-key': { setting: 'anthropic_api_key', env: 'ANTHROPIC_API_KEY' },
  'openai-key': { setting: 'openai_key', env: 'OPENAI_API_KEY' },
}

/** What an AI account is called: its backend, as the AI switch says it. */
export function aiName(a: Pick<AiAccount, 'backend' | 'this_computer'>): string {
  const label = a.backend ? (BACKEND_LABEL[a.backend] ?? a.backend) : 'The built-in AI'
  return a.this_computer ? `${label} on this computer` : label
}
