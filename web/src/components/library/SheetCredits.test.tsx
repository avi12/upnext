import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import type { GameView } from '../../types'
import { DataLine } from './SheetDetails'
import { Tags } from './SheetSections'

// RAWG's terms ask for a link to RAWG wherever its data shows (docs/THIRD_PARTY.md).
const RAWG_URL = 'https://rawg.io/games/portal-2'

function game(rawg: boolean): GameView {
  return {
    tags: ['Puzzle', 'Co-op'],
    tags_by_source: rawg ? { steam: ['Puzzle'], rawg: ['Co-op'] } : { steam: ['Puzzle', 'Co-op'] },
    rawg_url: rawg ? RAWG_URL : null,
    pcgw_url: null,
    wikidata_url: null,
    hltb_url: null,
    appid: null,
    protondb_tier: null,
  } as unknown as GameView
}

describe('the game sheet credits RAWG', () => {
  it('links RAWG in the "Info from" line when the game has RAWG data', () => {
    render(<DataLine game={game(true)} linuxActive={false} />)
    expect(screen.getByRole('link', { name: 'RAWG' }).getAttribute('href')).toBe(RAWG_URL)
  })

  it('links RAWG beside its tags in the breakdown by source', () => {
    render(<Tags game={game(true)} />)
    fireEvent.click(screen.getByRole('button', { name: 'By source' }))
    expect(screen.getByRole('link', { name: 'RAWG' }).getAttribute('href')).toBe(RAWG_URL)
  })

  it('shows no RAWG link for a game without RAWG data', () => {
    const { container } = render(
      <>
        <DataLine game={game(false)} linuxActive={false} />
        <Tags game={game(false)} />
      </>,
    )
    fireEvent.click(screen.getByRole('button', { name: 'By source' }))
    expect(container.querySelector('a[href^="https://rawg.io"]')).toBeNull()
  })
})
