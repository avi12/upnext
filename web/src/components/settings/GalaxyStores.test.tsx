import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import type { GalaxyIntegration, Settings } from '../../types'
import { GalaxyStoresRow } from './GalaxyStores'

const store = (platform: string, label: string, extra: Partial<GalaxyIntegration> = {}): GalaxyIntegration => ({
  platform,
  label,
  games: 2,
  last_added: '2026-10-01',
  connected: true,
  left_out: false,
  seen_at: 1_790_000_000,
  ...extra,
})

function settings(skip: string[], seen: GalaxyIntegration[]): Settings {
  return { galaxy_skip: skip, galaxy_integrations: seen } as unknown as Settings
}

describe('Settings, stores through Galaxy', () => {
  const seen = [
    store('gog', 'GOG'),
    store('origin', 'EA app', { games: 55, last_added: '2024-05-18' }),
    store('xboxone', 'Xbox', { games: 1, seen_at: 1_780_000_000 }),
    store('uplay', 'Ubisoft Connect', { connected: false, last_added: null }),
  ]

  it('lists every store but GOG, with what Galaxy says about it', () => {
    render(<GalaxyStoresRow s={settings(['xboxone'], seen)} save={vi.fn()} />)
    const boxes = screen.getAllByRole('checkbox') as HTMLInputElement[]
    expect(boxes.map((b) => b.checked)).toEqual([true, false, true])
    expect(screen.getByText('55 games · last added 2024-05-18')).toBeTruthy()
    expect(screen.getByText(/1 game · last added 2026-10-01 · not in Galaxy at the last sync; its games are kept/)).toBeTruthy()
    expect(screen.getByText('2 games · not connected in Galaxy')).toBeTruthy()
    expect(screen.queryByText('GOG')).toBeNull()
  })

  it('unticking a store leaves it out, ticking brings it back', () => {
    const save = vi.fn().mockResolvedValue(null)
    render(<GalaxyStoresRow s={settings(['xboxone'], seen)} save={save} />)
    fireEvent.click(screen.getByRole('checkbox', { name: /EA app/ }))
    expect(save).toHaveBeenLastCalledWith({ galaxy_skip: 'xboxone,origin' }, 'EA app games left out')
    fireEvent.click(screen.getByRole('checkbox', { name: /Xbox/ }))
    expect(save).toHaveBeenLastCalledWith({ galaxy_skip: '' }, 'Xbox games kept')
  })

  it('shows nothing before a Galaxy sync has found another store', () => {
    const { container } = render(<GalaxyStoresRow s={settings([], [store('gog', 'GOG')])} save={vi.fn()} />)
    expect(container.textContent).toBe('')
  })
})
