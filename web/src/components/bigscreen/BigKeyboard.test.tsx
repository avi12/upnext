import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { BigKeyboard } from './BigKeyboard'
import { offerAction } from './pad'

function Typing({ maxLength, onDone = () => {} }: { maxLength?: number; onDone?: () => void }) {
  const [value, setValue] = useState('')
  return (
    <>
      <span data-testid="value">{value}</span>
      <BigKeyboard label="In your own words" value={value} maxLength={maxLength} onChange={setValue} onDone={onDone} />
    </>
  )
}

const press = (name: string) => fireEvent.click(screen.getByRole('button', { name }))
const typed = () => screen.getByTestId('value').textContent
const offer = (a: Parameters<typeof offerAction>[0]) => {
  let took = false
  act(() => {
    took = offerAction(a)
  })
  return took
}

describe('the on-screen keyboard', () => {
  it('types letters, a capital after Shift, spaces and deletes', () => {
    render(<Typing />)
    for (const k of ['h', 'i', 'Space', 'Shift', 'A']) press(k)
    expect(typed()).toBe('hi A')
    // Shift lasts one letter.
    press('b')
    expect(typed()).toBe('hi Ab')
    press('Delete')
    expect(typed()).toBe('hi A')
  })

  it('stops at the field’s length', () => {
    render(<Typing maxLength={2} />)
    for (const k of ['1', '2', '3']) press(k)
    expect(typed()).toBe('12')
  })

  it('takes a pad’s X as delete, Y as space and Menu as Done', () => {
    const done = vi.fn()
    render(<Typing onDone={done} />)
    press('o')
    press('k')
    screen.getByRole('button', { name: 'q' }).focus()
    expect(offer('request')).toBe(true)
    expect(typed()).toBe('ok ')
    expect(offer('menu')).toBe(true)
    expect(typed()).toBe('ok')
    expect(offer('settings')).toBe(true)
    expect(done).toHaveBeenCalledOnce()
    // Moving isn't the keyboard's: the focus engine does it.
    expect(offer('right')).toBe(false)
  })
})
