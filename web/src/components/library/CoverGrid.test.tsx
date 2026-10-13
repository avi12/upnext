import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import type { GameView } from '../../types'
import { MOCK_GAME } from '../../test/mockGame'
import { CoverGrid } from './CoverGrid'

const game = (id: number, change: Partial<GameView>) => ({ ...MOCK_GAME, id, ...change }) as GameView

describe('The library’s covers', () => {
  it('say every status, with time played, and label the story length', () => {
    render(
      <CoverGrid
        linuxActive={false}
        onOpen={vi.fn()}
        items={[
          game(1, { name: 'A', status: 'untouched', playtime_minutes: 0, hltb_main: 30, review_pct: 97 }),
          game(2, { name: 'B', status: 'done', playtime_minutes: 21 * 60, hltb_main: 20, review_pct: null }),
          game(3, { name: 'C', status: 'burnt_out', playtime_minutes: 90, hltb_main: null, review_pct: 80 }),
        ]}
      />,
    )
    const lines = screen.getAllByRole('listitem').map((li) => [...li.querySelectorAll('.mono')].map((m) => m.textContent))
    expect(lines).toEqual([
      ['Not started', '30h story · 97%'],
      ['Beat it 21h', '20h story'],
      ['Burnt out 1.5h', '80%'],
    ])
  })

  it('show your rating after the status', () => {
    render(<CoverGrid linuxActive={false} onOpen={vi.fn()} items={[game(1, { status: 'done', playtime_minutes: 600, my_rating: 4 })]} />)
    expect(screen.getByText('★ 4').closest('.mono')?.textContent).toBe('Beat it 10h · ★ 4your rating: 4 of 5')
  })

  it('explain the Linux dot while Linux compatibility is on', () => {
    const { rerender } = render(<CoverGrid linuxActive={false} onOpen={vi.fn()} items={[game(1, { linux: 'platinum' })]} />)
    expect(screen.queryByText(/Linux rating/)).toBeNull()
    rerender(<CoverGrid linuxActive onOpen={vi.fn()} items={[game(1, { linux: 'platinum' })]} />)
    expect(screen.getByText('The dot is the Linux rating, from ProtonDB:')).toBeTruthy()
    expect(screen.getByText('runs well')).toBeTruthy()
  })
})
