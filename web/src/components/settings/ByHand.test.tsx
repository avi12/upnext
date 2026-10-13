import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { ByHandLists } from '../../types'
import { ToastProvider } from '../Toast'
import { ByHandRow } from './ByHand'

const lists = (origin: string[]): ByHandLists => ({
  stores: [
    { store: 'origin', label: 'EA app', titles: origin },
    { store: 'uplay', label: 'Ubisoft Connect', titles: [] },
    { store: 'battlenet', label: 'Battle.net', titles: [] },
    { store: 'xboxone', label: 'Xbox', titles: [] },
  ],
})

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <ByHandRow />
      </ToastProvider>
    </QueryClientProvider>,
  )
}

describe('Settings, games added by hand', () => {
  afterEach(() => vi.restoreAllMocks())

  it('shows each store’s list and saves the one being edited', async () => {
    vi.spyOn(api, 'byHand').mockResolvedValue(lists(['Dead Space']))
    const save = vi.spyOn(api, 'byHandSave').mockResolvedValue({ total: 2, added: 1, removed: 0, lists: lists(['Dead Space', 'Titanfall 2']) })
    mount()
    expect(await screen.findByText('EA app 1')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Change' }))
    const box = screen.getByRole('textbox', { name: 'EA app games, one a line' }) as HTMLTextAreaElement
    expect(box.value).toBe('Dead Space')
    const button = screen.getByRole('button', { name: 'Save list' }) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    fireEvent.change(box, { target: { value: 'Dead Space\nTitanfall 2' } })
    fireEvent.click(button)
    await waitFor(() => expect(save).toHaveBeenCalledWith('origin', ['Dead Space\nTitanfall 2']))
    expect(await screen.findByText('EA app 2')).toBeTruthy()
  })

  it('says none were added, and switches to another store’s list', async () => {
    vi.spyOn(api, 'byHand').mockResolvedValue(lists([]))
    mount()
    expect(await screen.findByText('None')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Add games' }))
    fireEvent.click(screen.getByRole('radio', { name: 'Ubisoft Connect' }))
    expect(screen.getByRole('textbox', { name: 'Ubisoft Connect games, one a line' })).toBeTruthy()
  })
})
