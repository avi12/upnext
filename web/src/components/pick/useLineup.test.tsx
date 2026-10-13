import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { OwnedPick, PickBatch } from '../../types'
import { ToastContext, type ToastApi } from '../useToast'
import { useLineup } from './useLineup'

afterEach(() => vi.restoreAllMocks())

const pick = { id: 4, slot: 0, game: { id: 4, name: 'Hollow Knight' } } as unknown as OwnedPick
const batch = { batch_id: 'b1', prompt: '', picks: [pick], shown: 1 } as unknown as PickBatch

describe('useLineup', () => {
  it('Never recommend this says where the game can be brought back, beside its Undo (B-WEB-8)', async () => {
    vi.spyOn(api, 'pickFeedback').mockResolvedValue(batch)
    const success = vi.fn()
    const toast = { push: vi.fn(), success, error: vi.fn(), info: vi.fn() } as unknown as ToastApi
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>
        <ToastContext.Provider value={toast}>{children}</ToastContext.Provider>
      </QueryClientProvider>
    )
    const { result } = renderHook(() => useLineup(batch), { wrapper })
    act(() => result.current.act.mutate({ pick, action: 'not_for_me' }))
    await waitFor(() => expect(success).toHaveBeenCalled())
    const [title, detail, action] = success.mock.calls[0]
    expect(title).toBe("Hollow Knight won't come up again")
    expect(detail).toBe('Your Library lists it under Not for me, if you change your mind.')
    expect(action.label).toBe('Undo')
  })
})
