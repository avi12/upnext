import { describe, expect, it, vi } from 'vitest'
import { useState } from 'react'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { SentenceChoice, SentenceMenu } from './SentenceMenu'

function Count() {
  const [n, setN] = useState<'3' | '5'>('3')
  return (
    <SentenceChoice
      label="Number of picks"
      value={n}
      onChange={setN}
      options={[
        { value: '3', label: 'three' },
        { value: '5', label: 'five' },
      ]}
    />
  )
}

describe('SentenceMenu', () => {
  it('opens on the word, checks the current choice and closes after one', async () => {
    const user = userEvent.setup()
    render(<Count />)
    const button = screen.getByRole('button', { name: 'Number of picks: three' })
    expect(button.getAttribute('aria-expanded')).toBe('false')

    await user.click(button)
    expect(button.getAttribute('aria-expanded')).toBe('true')
    const three = screen.getByRole('menuitemradio', { name: 'three' })
    expect(three.getAttribute('aria-checked')).toBe('true')
    expect(document.activeElement).toBe(three)

    await user.click(screen.getByRole('menuitemradio', { name: 'five' }))
    expect(screen.queryByRole('menu')).toBeNull()
    expect(screen.getByRole('button', { name: 'Number of picks: five' })).toBe(document.activeElement)
  })

  it('moves with the arrow keys and closes on Esc', async () => {
    const user = userEvent.setup()
    render(<Count />)
    screen.getByRole('button').focus()
    await user.keyboard('{ArrowDown}')
    expect(screen.getByRole('menu', { name: 'Number of picks' })).toBeTruthy()
    expect(document.activeElement?.textContent).toBe('three')
    await user.keyboard('{ArrowDown}')
    expect(document.activeElement?.textContent).toBe('five')
    await user.keyboard('{ArrowDown}')
    expect(document.activeElement?.textContent).toBe('three')
    await user.keyboard('{End}')
    expect(document.activeElement?.textContent).toBe('five')
    await user.keyboard('{Escape}')
    expect(screen.queryByRole('menu')).toBeNull()
    expect(document.activeElement).toBe(screen.getByRole('button'))
  })

  it('stays open for a multiple choice, and closes on a click outside', async () => {
    const user = userEvent.setup()
    const steam = vi.fn()
    const gog = vi.fn()
    render(
      <div>
        <p>outside</p>
        <SentenceMenu
          label="every store"
          title="Stores"
          multi
          options={[
            { key: 'steam', label: 'Steam', checked: true, onSelect: steam },
            { key: 'gog', label: 'GOG', checked: false, onSelect: gog },
          ]}
        />
      </div>,
    )
    await user.click(screen.getByRole('button', { name: 'Stores: every store' }))
    await user.click(screen.getByRole('menuitemcheckbox', { name: 'GOG' }))
    expect(gog).toHaveBeenCalledOnce()
    expect(screen.getByRole('menu')).toBeTruthy()
    await user.click(screen.getByText('outside'))
    expect(screen.queryByRole('menu')).toBeNull()
    expect(steam).not.toHaveBeenCalled()
  })
})

describe('SentenceChoice with nothing chosen', () => {
  it('shows the placeholder as a blank, and the choice once there is one', async () => {
    function Length() {
      const [v, setV] = useState('')
      return (
        <SentenceChoice
          label="How long on a weeknight"
          value={v}
          placeholder="x hours"
          onChange={setV}
          options={[
            { value: '', label: 'not said' },
            { value: '60', label: 'an hour' },
          ]}
        />
      )
    }
    const user = userEvent.setup()
    render(<Length />)
    const blank = screen.getByRole('button', { name: 'How long on a weeknight: not said' })
    expect(blank.textContent).toBe('x hours')
    expect(blank.className).toMatch(/sentence-blank/)
    await user.click(blank)
    await user.click(screen.getByRole('menuitemradio', { name: 'an hour' }))
    const set = screen.getByRole('button', { name: 'How long on a weeknight: an hour' })
    expect(set.textContent).toBe('an hour')
    expect(set.className).not.toMatch(/sentence-blank/)
  })
})
