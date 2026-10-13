import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import type { Settings } from '../../types'
import { WindowsSection } from './Windows'

function settings(change: Partial<Settings>): Settings {
  return { os: 'windows', keep_running: true, start_with_windows: false, ...change } as Settings
}

describe('Settings, Windows', () => {
  it('shows both switches as the settings are', () => {
    render(<WindowsSection s={settings({})} save={vi.fn()} />)
    expect((screen.getByRole('checkbox', { name: /Keep running in the background/ }) as HTMLInputElement).checked).toBe(true)
    expect((screen.getByRole('checkbox', { name: /Start with Windows/ }) as HTMLInputElement).checked).toBe(false)
  })

  it('saves each switch as its own setting', () => {
    const save = vi.fn().mockResolvedValue(null)
    render(<WindowsSection s={settings({})} save={save} />)
    fireEvent.click(screen.getByRole('checkbox', { name: /Start with Windows/ }))
    expect(save).toHaveBeenLastCalledWith({ start_with_windows: true }, 'UpNext starts with Windows')
    fireEvent.click(screen.getByRole('checkbox', { name: /Keep running in the background/ }))
    expect(save).toHaveBeenLastCalledWith({ keep_running: false }, 'Closing the window stops UpNext')
  })
})
