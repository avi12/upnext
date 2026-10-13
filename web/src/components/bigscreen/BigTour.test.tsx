import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { BigTour } from './BigTour'
import { PadStyleContext } from './pad'

describe('the big-screen tour', () => {
  it('goes stop by stop, naming the buttons as the pad in hand does', () => {
    const done = vi.fn()
    render(
      <PadStyleContext.Provider value="playstation">
        <BigTour onClose={done} />
      </PadStyleContext.Provider>,
    )
    expect(screen.getByRole('dialog', { name: 'Say what you’re up for' })).toBeTruthy()
    // PlayStation's triangle opens the request, not Xbox's Y.
    expect(screen.getAllByText('△').length).toBeGreaterThan(0)
    for (let i = 0; i < 3; i++) fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    expect(screen.getByText('4 of 4')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Back' }))
    expect(screen.getByText('3 of 4')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    fireEvent.click(screen.getByRole('button', { name: 'Done' }))
    expect(done).toHaveBeenCalledTimes(1)
  })
})
