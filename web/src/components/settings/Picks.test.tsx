import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { Settings } from '../../types'
import { ToastProvider } from '../Toast'
import { AiChooser } from './Picks'

function mount(url: string, models: Awaited<ReturnType<typeof api.llmModels>>) {
  vi.spyOn(api, 'llmModels').mockResolvedValue(models)
  const s = {
    llm_backend: 'openai',
    llm_backends: ['claude-cli', 'api-key', 'openai'],
    openai_base_url: url,
    openai_api_key_set: false,
    openai_model: null,
    openai_digest_model: null,
    openai_thinking: 'auto',
    listen: { this_computer: true },
  } as unknown as Settings
  const save = vi.fn().mockResolvedValue(null)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <AiChooser s={s} save={save} />
      </ToastProvider>
    </QueryClientProvider>,
  )
  return save
}

describe('Settings, another provider', () => {
  afterEach(() => vi.restoreAllMocks())

  it('offers your own server first, and choosing it empties the address to type', async () => {
    const save = mount('http://localhost:11434/v1', { ok: false, models: [], error: 'cannot reach http://localhost:11434/v1: refused' })
    const provider = screen.getByRole('combobox', { name: 'Provider' }) as HTMLSelectElement
    expect(provider.value).toBe('ollama')
    expect(provider.options[0].text).toBe('Your own server (vLLM, llama.cpp, any OpenAI-compatible)')
    expect(provider.options[0].disabled).toBe(false)
    expect(await screen.findByText('Nothing answering at http://localhost:11434/v1 yet: start the server there, or choose another.')).toBeTruthy()

    fireEvent.change(provider, { target: { value: 'custom' } })
    expect(provider.value).toBe('custom')
    const address = screen.getByRole('textbox', { name: 'Address' }) as HTMLInputElement
    expect(address.value).toBe('')
    expect(address.placeholder).toBe('http://192.168.0.42:8000/v1')
    expect(screen.queryByText(/Nothing answering/)).toBeNull()
    expect(save).not.toHaveBeenCalled()

    fireEvent.change(address, { target: { value: 'http://192.168.0.42:8000/v1' } })
    fireEvent.blur(address)
    await waitFor(() => expect(save).toHaveBeenCalledWith({ openai_base_url: 'http://192.168.0.42:8000/v1' }, 'AI server', { inline: true }))
  })

  it('shows any other listing error as it is', async () => {
    mount('https://openrouter.ai/api/v1', { ok: false, models: [], error: 'HTTP 401: no key' })
    expect(await screen.findByText("Couldn't list models: HTTP 401: no key")).toBeTruthy()
  })
})

function mountChooser(over: Partial<Settings>, purpose?: 'summaries') {
  vi.spyOn(api, 'llmModels').mockResolvedValue({ ok: true, models: [] })
  const llmTest = vi.spyOn(api, 'llmTest').mockResolvedValue({ ok: true, backend: 'openai', model: 'qwen-small' } as never)
  const s = {
    llm_backend: 'claude-cli',
    llm_backends: ['claude-cli', 'api-key', 'openai'],
    digest_backend: 'same',
    openai_base_url: 'http://192.168.0.42:8000/v1',
    openai_api_key_set: false,
    openai_model: null,
    openai_digest_model: null,
    openai_thinking: 'auto',
    listen: { this_computer: true },
    ...over,
  } as unknown as Settings
  const save = vi.fn().mockResolvedValue(null)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <AiChooser s={s} save={save} purpose={purpose} />
      </ToastProvider>
    </QueryClientProvider>,
  )
  return { save, llmTest }
}

describe('Settings, an AI for summaries', () => {
  afterEach(() => vi.restoreAllMocks())

  it('starts as the same AI as picks, and offers the other backends', () => {
    const { save } = mountChooser({}, 'summaries')
    const group = screen.getByRole('radiogroup', { name: 'AI for summaries' })
    const choices = Array.from(group.querySelectorAll('[role=radio]')).map((b) => b.textContent)
    expect(choices).toEqual(['Same as picks', 'Anthropic API key', 'Another provider'])
    expect(screen.getByRole('radio', { name: 'Same as picks' }).getAttribute('aria-checked')).toBe('true')
    expect(screen.queryByRole('button', { name: 'Test' })).toBeNull()

    fireEvent.click(screen.getByRole('radio', { name: 'Another provider' }))
    expect(save).toHaveBeenCalledWith({ digest_backend: 'openai' }, 'Summaries: Another provider')
  })

  it("shows its backend's summaries model alone, and tests that backend", async () => {
    const { llmTest } = mountChooser({ digest_backend: 'openai', openai_model: 'qwen-big' }, 'summaries')
    expect(screen.getByRole('radio', { name: 'Another provider' }).getAttribute('aria-checked')).toBe('true')
    const model = screen.getByRole('combobox', { name: 'Summaries model' }) as HTMLInputElement
    expect(model.placeholder).toBe("The server's model, qwen-big")
    expect(screen.queryByRole('combobox', { name: 'Picks model' })).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Test' }))
    await waitFor(() => expect(llmTest).toHaveBeenCalledWith('summaries'))
  })

  it("leaves the summaries model out of the picks' fields while summaries go elsewhere", () => {
    mountChooser({ llm_backend: 'openai', digest_backend: 'api-key' })
    expect(screen.getByRole('combobox', { name: 'Picks model' })).toBeTruthy()
    expect(screen.queryByRole('combobox', { name: 'Summaries model' })).toBeNull()
  })
})
