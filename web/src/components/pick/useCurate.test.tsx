import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { PickBatch, PickResult, Routines } from '../../types'
import { ToastContext, type ToastApi } from '../useToast'
import { useCurate, type CurateRequest } from './useCurate'

afterEach(() => vi.restoreAllMocks())

const lineup = (id: string) => ({ batch_id: id, prompt: 'cosy', picks: [], shown: 0 }) as unknown as PickBatch
const ask: CurateRequest = { prompt: 'cosy', count: 3, exclude: [], why: [], withoutAi: false, routineId: null }

function setup(last: PickBatch | null, opts: Parameters<typeof useCurate>[0] = {}) {
  vi.spyOn(api, 'history').mockResolvedValue(last ? [last] : [])
  vi.spyOn(api, 'routines').mockResolvedValue({ routines: [{ id: 7, name: 'Weeknights' }, { id: 8, name: 'Weekends' }], now: 7 } as unknown as Routines)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const invalidate = vi.spyOn(client, 'invalidateQueries')
  const toast = { push: vi.fn(), success: vi.fn(), error: vi.fn(), info: vi.fn() } as unknown as ToastApi
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <ToastContext.Provider value={toast}>{children}</ToastContext.Provider>
    </QueryClientProvider>
  )
  const hook = renderHook(() => useCurate(opts), { wrapper })
  return { ...hook, invalidate }
}

describe('useCurate (R-WEB-3)', () => {
  it('asks once while a request is out, however often it is asked', async () => {
    let answer: (r: PickResult) => void = () => {}
    const pick = vi.spyOn(api, 'pick').mockReturnValue(new Promise((r) => (answer = r)))
    const { result } = setup(null)
    await waitFor(() => expect(result.current.last.isSuccess).toBe(true))
    act(() => result.current.curate(ask))
    await waitFor(() => expect(result.current.m.isPending).toBe(true))
    act(() => result.current.curate(ask))
    act(() => result.current.curate({ ...ask, withoutAi: true }))
    expect(pick).toHaveBeenCalledTimes(1)
    await act(async () => answer(lineup('b2') as PickResult))
    await waitFor(() => expect(result.current.m.isPending).toBe(false))
    act(() => result.current.curate(ask))
    await waitFor(() => expect(pick).toHaveBeenCalledTimes(2))
  })

  it('sends the request as asked, with the sources and the controller input', async () => {
    const pick = vi.spyOn(api, 'pick').mockResolvedValue(lineup('b2') as PickResult)
    const { result } = setup(null, { sources: ['steam'], input: 'controller' })
    act(() => result.current.curate({ ...ask, exclude: [4], why: ['Too long'], routineId: 7 }))
    await waitFor(() => expect(pick).toHaveBeenCalled())
    expect(pick).toHaveBeenCalledWith('cosy', 3, [4], ['steam'], ['Too long'], false, 7, undefined, 'controller')
    act(() => result.current.curate({ ...ask, prompt: '', count: 1, random: true }))
    await waitFor(() => expect(pick).toHaveBeenCalledTimes(2))
    expect(pick).toHaveBeenLastCalledWith('', 1, [], ['steam'], [], false, null, 'random', 'controller')
  })

  it('shows the new lineup, has History read again, and tells the page', async () => {
    vi.spyOn(api, 'pick').mockResolvedValue(lineup('b2') as PickResult)
    const onPicked = vi.fn()
    const { result, invalidate } = setup(lineup('b1'), { onPicked })
    await waitFor(() => expect(result.current.batch?.batch_id).toBe('b1'))
    act(() => result.current.curate(ask))
    await waitFor(() => expect(result.current.batch?.batch_id).toBe('b2'))
    expect(onPicked).toHaveBeenCalledWith(expect.objectContaining({ batch_id: 'b2' }))
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['history', 'list'] })
  })

  it('a failure keeps the lineup on screen and tells the page which one it was', async () => {
    const failure = new ApiError(502, 'the AI is down')
    vi.spyOn(api, 'pick').mockRejectedValue(failure)
    const onFailed = vi.fn()
    const { result } = setup(lineup('b1'), { onFailed })
    await waitFor(() => expect(result.current.batch?.batch_id).toBe('b1'))
    act(() => result.current.curate(ask))
    await waitFor(() => expect(onFailed).toHaveBeenCalled())
    expect(onFailed).toHaveBeenCalledWith(failure, ask, expect.objectContaining({ batch_id: 'b1' }))
    expect(result.current.batch?.batch_id).toBe('b1')
  })

  it('a failure with no lineup on screen says so', async () => {
    vi.spyOn(api, 'pick').mockRejectedValue(new ApiError(400, 'no candidates: nothing to pick from'))
    const onFailed = vi.fn()
    const { result } = setup(null, { onFailed })
    await waitFor(() => expect(result.current.last.isSuccess).toBe(true))
    act(() => result.current.curate(ask))
    await waitFor(() => expect(onFailed).toHaveBeenCalled())
    expect(onFailed.mock.calls[0][2]).toBeNull()
  })

  it("hides a lineup the page cleared away", async () => {
    const { result } = setup(lineup('b1'), { hide: 'b1' })
    await waitFor(() => expect(result.current.last.data?.batch_id).toBe('b1'))
    expect(result.current.batch).toBeNull()
  })

  it('takes the routine that fits now until one is chosen, and none when none is', async () => {
    const { result } = setup(null)
    await waitFor(() => expect(result.current.routine?.id).toBe(7))
    act(() => result.current.setRoutineChoice(8))
    expect(result.current.routine?.id).toBe(8)
    act(() => result.current.setRoutineChoice(null))
    expect(result.current.routine).toBeNull()
  })
})
