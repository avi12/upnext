import { useRef } from 'react'
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useTabTrap } from './useTabTrap'

function Trap({ name }: { name: string }) {
  const root = useRef<HTMLDivElement>(null)
  useTabTrap(root)
  return (
    <div ref={root}>
      <button type="button">{name} one</button>
      <button type="button">{name} two</button>
    </div>
  )
}

describe('useTabTrap', () => {
  it('keeps Tab and Shift+Tab inside, and brings focus from outside in', async () => {
    render(
      <>
        <button type="button">Behind</button>
        <Trap name="Dialog" />
      </>,
    )
    const user = userEvent.setup()
    screen.getByRole('button', { name: 'Behind' }).focus()
    await user.tab()
    expect(document.activeElement?.textContent).toBe('Dialog one')
    await user.tab()
    await user.tab()
    expect(document.activeElement?.textContent).toBe('Dialog one')
    await user.tab({ shift: true })
    expect(document.activeElement?.textContent).toBe('Dialog two')
  })

  it('with two up at once, the one holding the focus keeps it, and focus in neither goes to the newer', async () => {
    render(
      <>
        <button type="button">Behind</button>
        <Trap name="Older" />
        <Trap name="Newer" />
      </>,
    )
    const user = userEvent.setup()
    screen.getByRole('button', { name: 'Newer one' }).focus()
    await user.tab()
    await user.tab()
    expect(document.activeElement?.textContent).toBe('Newer one')
    screen.getByRole('button', { name: 'Older two' }).focus()
    await user.tab()
    expect(document.activeElement?.textContent).toBe('Older one')
    screen.getByRole('button', { name: 'Behind' }).focus()
    await user.tab()
    expect(document.activeElement?.textContent).toBe('Newer one')
  })
})
