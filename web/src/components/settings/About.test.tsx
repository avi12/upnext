import { afterEach, describe, expect, it, vi } from 'vitest'
import { render as mount, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import { CREDITS } from '../../lib/credits'
import { buildText } from '../../lib/build'
import { AboutSection } from './About'

function render(node: React.ReactNode, build: string | null = 'abc1234') {
  vi.spyOn(api, 'health').mockResolvedValue({ app: 'upnext', version: '0.1.0', build, container: false })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(<QueryClientProvider client={client}>{node}</QueryClientProvider>)
}

describe('Settings › About', () => {
  afterEach(() => vi.restoreAllMocks())

  it('names the version and the build it runs', async () => {
    render(<AboutSection />)
    expect(await screen.findByText('UpNext 0.1.0, build abc1234')).toBeTruthy()
    expect(buildText({ version: '0.1.0', build: null })).toBe('UpNext 0.1.0')
  })

  it('lists every data source with a link', () => {
    render(<AboutSection />)
    const list = screen.getByRole('list', { name: 'Data sources' })
    expect(within(list).getAllByRole('listitem')).toHaveLength(CREDITS.length)
    for (const c of CREDITS) expect(within(list).getByRole('link', { name: c.name }).getAttribute('href')).toBe(c.url)
  })

  // The attributions their terms ask for (docs/THIRD_PARTY.md).
  it('credits PCGamingWiki under CC BY-NC-SA and links RAWG', () => {
    render(<AboutSection />)
    expect(screen.getByText(/PCGamingWiki content is available under/)).toBeTruthy()
    expect(screen.getByRole('link', { name: 'CC BY-NC-SA 3.0' }).getAttribute('href')).toBe('https://creativecommons.org/licenses/by-nc-sa/3.0/')
    expect(screen.getByRole('link', { name: 'RAWG' }).getAttribute('href')).toBe('https://rawg.io')
  })

  it('links the website and the source', () => {
    render(<AboutSection />)
    expect(screen.getByRole('link', { name: 'upnext.games' }).getAttribute('href')).toBe('https://upnext.games')
    expect(screen.getByRole('link', { name: 'GitHub' }).getAttribute('href')).toBe('https://github.com/zytocc/upnext')
  })

  // Served by the program itself (THIRD_PARTY_NOTICES.txt).
  it('links the third-party notices', () => {
    render(<AboutSection />)
    expect(screen.getByRole('link', { name: 'third-party notices' }).getAttribute('href')).toBe('/third-party-notices.txt')
  })

  it('says it is not affiliated with the services', () => {
    render(<AboutSection />)
    expect(screen.getByText(/isn't affiliated with any of them/)).toBeTruthy()
  })
})
