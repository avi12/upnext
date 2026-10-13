import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { Place } from './Place'
import { placeText } from '../lib/place'

describe('Places in the app', () => {
  it('read as a path and link to their section of Settings', () => {
    render(
      <MemoryRouter>
        <p>
          It runs on a schedule you set in <Place section="Jobs" />.
        </p>
        <Place section="Library data" />
      </MemoryRouter>,
    )
    expect(screen.getByRole('link', { name: 'Settings › Jobs' }).getAttribute('href')).toBe('/settings#jobs')
    expect(screen.getByRole('link', { name: 'Settings › Library data' }).getAttribute('href')).toBe('/settings#data')
    expect(placeText('Devices and API')).toBe('Settings › Devices and API')
  })
})
