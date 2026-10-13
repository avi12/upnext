// The page-side helpers below run in the browser, so this file sees its types.
/// <reference lib="dom" />
import { expect, test, type Page } from '@playwright/test'

// Big screen at a Steam Deck's 1280×800: Curate, the lineup, a
// game page and its Play link, first by keyboard alone, then with a gamepad
// scripted through the Gamepad API.

/** Every visible piece of text smaller than `min` px, with its size. */
async function smallText(page: Page, min: number): Promise<string[]> {
  return page.evaluate((min) => {
    const out: string[] = []
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const text = n.textContent?.trim()
      const el = n.parentElement
      if (!text || !el) continue
      const r = el.getBoundingClientRect()
      const style = getComputedStyle(el)
      if (r.width === 0 || r.height === 0 || style.visibility === 'hidden' || el.closest('.sr-only, [aria-hidden="true"]')) continue
      const size = parseFloat(style.fontSize)
      if (size < min) out.push(`${size.toFixed(1)}px: ${text.slice(0, 40)}`)
    }
    return out
  }, min)
}

/** The name of whatever has the focus. */
function focused(page: Page) {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null
    return el && el !== document.body ? (el.getAttribute('aria-label') ?? el.textContent ?? '').trim() : ''
  })
}

test.describe('big screen', () => {
  test.use({ viewport: { width: 1280, height: 800 } })

  test('a keyboard alone curates, opens a game and finds Play', async ({ page }, info) => {
    const errors: string[] = []
    page.on('pageerror', (e) => errors.push(e.message))
    await page.goto('/?big=1')
    await expect(page.getByRole('navigation', { name: 'Big screen' })).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.hasAttribute('data-bigscreen'))).toBe(true)

    // / (Y on a pad) brings up the request with the focus on its first chip,
    // whether or not an earlier test left a lineup.
    await page.keyboard.press('/')
    const request = page.getByRole('region', { name: 'Request' })
    await expect(request.getByRole('button', { name: 'Half an hour' })).toBeFocused()
    await page.keyboard.press('ArrowRight')
    await page.keyboard.press('Enter')
    await expect(request.getByRole('button', { name: 'An hour', pressed: true })).toBeVisible()
    expect(await smallText(page, 14)).toEqual([])

    // Down to Curate, and the lineup takes the focus on its first card.
    const curate = page.getByRole('button', { name: 'Curate', exact: true })
    for (let i = 0; i < 8 && !(await curate.evaluate((el) => el === document.activeElement)); i++) {
      const inField = await page.evaluate(() => document.activeElement?.tagName === 'INPUT')
      await page.keyboard.press(inField ? 'ArrowRight' : 'ArrowDown')
    }
    await expect(curate).toBeFocused()
    await page.keyboard.press('Enter')
    const lineup = page.getByRole('region', { name: 'Lineup' })
    await expect(lineup.getByRole('article')).toHaveCount(3)
    const first = (await lineup.getByRole('article').first().getAttribute('aria-labelledby'))!
    const firstName = (await page.locator(`[id="${first}"]`).innerText()).trim()
    await expect.poll(() => focused(page)).toContain(firstName)
    await page.screenshot({ path: info.outputPath('curate-1280x800.png') })
    expect(await smallText(page, 14)).toEqual([])

    // Right moves along the lineup; Enter opens the game page.
    await page.keyboard.press('ArrowRight')
    const second = (await lineup.getByRole('article').nth(1).getAttribute('aria-labelledby'))!
    const secondName = (await page.locator(`[id="${second}"]`).innerText()).trim()
    await expect.poll(() => focused(page)).toContain(secondName)
    await page.keyboard.press('Enter')
    await expect(page).toHaveURL(/\/game\/\d+$/)
    await expect(page.getByRole('heading', { level: 1, name: secondName })).toBeVisible()
    const play = page.getByRole('link', { name: 'Play', exact: true })
    await expect(play).toBeFocused()
    expect(await play.getAttribute('href')).toMatch(/^[a-z]+:\/\//)
    await page.screenshot({ path: info.outputPath('game-1280x800.png') })
    expect(await smallText(page, 14)).toEqual([])

    // Mark as… opens a menu that holds the focus; Escape closes it, then
    // goes back a page.
    await page.getByRole('button', { name: /^Mark as|^Marked/ }).focus()
    await page.keyboard.press('Enter')
    const menu = page.getByRole('dialog')
    await expect(menu).toBeVisible()
    await page.keyboard.press('ArrowDown')
    expect(await menu.evaluate((d) => d.contains(document.activeElement))).toBe(true)
    await page.keyboard.press('Escape')
    await expect(menu).toBeHidden()
    await page.keyboard.press('Escape')
    await expect(page).toHaveURL(/\/$/)

    // The bumpers' keys step through the pages, and each page keeps to 14 px.
    for (const [path, name] of [
      ['/library', 'Library'],
      ['/lists', 'Lists'],
      ['/versus', 'Help me decide'],
      ['/settings', 'Settings'],
    ]) {
      await page.keyboard.press(']')
      await expect(page).toHaveURL(new RegExp(`${path}$`))
      await expect(page.getByRole('link', { name, exact: true })).toHaveAttribute('aria-current', 'page')
      await expect(page.locator('main')).toContainText(/\w{3}/)
      expect(await smallText(page, 14), path).toEqual([])
    }

    // A page made for a mouse offers the way out, and leaving keeps the page.
    await page.goto('/history')
    await page.getByRole('button', { name: 'Leave big screen' }).click()
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible()
    await expect(page).toHaveURL(/\/history$/)
    expect(await page.evaluate(() => document.documentElement.hasAttribute('data-bigscreen'))).toBe(false)
    expect(errors).toEqual([])
  })

  // Enter pressed again in the request while the AI works sends nothing
  // more (code health B-WEB-3): each request is a call to the AI, and two
  // made two lineups in History.
  test('Enter twice in the request asks the AI once', async ({ page }) => {
    let posts = 0
    // Held a moment, as a real model takes its time.
    await page.route('**/api/v1/picks', async (route) => {
      if (route.request().method() === 'POST') {
        posts++
        await new Promise((r) => setTimeout(r, 1500))
      }
      await route.continue()
    })
    await page.goto('/?big=1')
    await expect(page.getByRole('navigation', { name: 'Big screen' })).toBeVisible()
    await page.keyboard.press('/')
    const field = page.getByLabel('In your own words')
    await field.click()
    await field.fill('something short')
    await field.press('Enter')
    await expect(page.getByText('Curating a lineup from your library…')).toBeVisible()
    await field.press('Enter')
    await expect(page.getByRole('region', { name: 'Lineup' }).getByRole('article')).toHaveCount(3)
    expect(posts).toBe(1)
  })

  test('a gamepad drives it, with its own button names', async ({ page, request }) => {
    // A lineup to start from, made the way the UI makes one.
    expect((await request.post('/api/v1/picks', { data: { prompt: 'A quiet hour', count: 3 } })).ok()).toBe(true)
    // A standard-mapping pad that the test presses buttons on.
    await page.addInitScript(() => {
      const buttons = Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 }))
      const pad = {
        id: 'Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)',
        index: 0,
        connected: true,
        mapping: 'standard',
        timestamp: 0,
        axes: [0, 0, 0, 0],
        buttons,
      }
      Object.defineProperty(navigator, 'getGamepads', { value: () => [pad] })
      ;(window as unknown as { __pad: unknown }).__pad = {
        down: (i: number) => (buttons[i] = { pressed: true, touched: true, value: 1 }),
        up: (i: number) => (buttons[i] = { pressed: false, touched: false, value: 0 }),
      }
    })
    const press = async (button: number) => {
      await page.evaluate((b) => (window as unknown as { __pad: { down: (i: number) => void } }).__pad.down(b), button)
      await page.waitForTimeout(80)
      await page.evaluate((b) => (window as unknown as { __pad: { up: (i: number) => void } }).__pad.up(b), button)
      await page.waitForTimeout(80)
    }
    const A = 0
    const B = 1
    const X = 2
    const RB = 5
    const RIGHT = 15

    await page.goto('/?big=1')
    const lineup = page.getByRole('region', { name: 'Lineup' })
    await expect(lineup.getByRole('article')).toHaveCount(3)
    const first = (await lineup.getByRole('article').first().getAttribute('aria-labelledby'))!
    await expect.poll(() => focused(page)).toContain((await page.locator(`[id="${first}"]`).innerText()).trim())
    await press(RIGHT)
    // The hints name the pad's buttons once it has been used.
    await expect(page.locator('kbd', { hasText: /^RB$/ })).toBeVisible()
    const second = (await lineup.getByRole('article').nth(1).getAttribute('aria-labelledby'))!
    const name = (await page.locator(`[id="${second}"]`).innerText()).trim()
    await expect.poll(() => focused(page)).toContain(name)

    // X opens the card's menu; B closes it.
    await press(X)
    await expect(page.getByRole('dialog', { name })).toBeVisible()
    await press(B)
    await expect(page.getByRole('dialog')).toBeHidden()

    // A opens the game page, where Play has the focus; B comes back.
    await press(A)
    await expect(page).toHaveURL(/\/game\/\d+$/)
    await expect(page.getByRole('link', { name: 'Play', exact: true })).toBeFocused()
    await press(B)
    await expect(page).toHaveURL(/:\d+\/(\?big=1)?$/)

    // RB is the next page.
    await press(RB)
    await expect(page).toHaveURL(/\/library$/)
  })

  test('the layout holds from a Deck to a 4K TV', async ({ page }, info) => {
    for (const [w, h] of [
      [1280, 800],
      [1920, 1080],
      [3840, 2160],
    ]) {
      await page.setViewportSize({ width: w, height: h })
      await page.goto('/?big=1')
      await expect(page.getByRole('region', { name: 'Lineup' })).toBeVisible()
      // Nothing wider than the window.
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
      await page.screenshot({ path: info.outputPath(`curate-${w}x${h}.png`) })
    }
  })
})

test('the UI installs: a manifest with its icons, open before pairing', async ({ page, request }) => {
  await page.goto('/')
  const href = await page.locator('link[rel="manifest"]').getAttribute('href')
  expect(href).toBe('/manifest.webmanifest')
  const res = await request.get(href!)
  expect(res.headers()['content-type']).toBe('application/manifest+json')
  const manifest = (await res.json()) as { start_url: string; display: string; icons: { src: string; sizes: string; purpose?: string }[]; shortcuts: { url: string }[] }
  expect([manifest.start_url, manifest.display]).toEqual(['/', 'standalone'])
  expect(manifest.icons.map((i) => `${i.sizes} ${i.purpose}`)).toEqual(['192x192 any', '512x512 any', '512x512 maskable'])
  expect(manifest.shortcuts.map((s) => s.url)).toEqual(['/', '/lists'])
  const touch = await page.locator('link[rel="apple-touch-icon"]').getAttribute('href')
  for (const src of [...manifest.icons.map((i) => i.src), touch!]) {
    const icon = await request.get(src)
    expect([src, icon.status(), icon.headers()['content-type']]).toEqual([src, 200, 'image/png'])
  }
})

test('in the app window big screen has the window buttons in its header, and the focus never reaches them', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await page.addInitScript(`
    window.upnextApp = true
    window.upnextFrame = { os: 'linux' }
    window.ipc = { postMessage: () => {} }
  `)
  await page.goto('/library?big=1')
  await expect(page.getByTestId('title-bar')).toHaveCount(0)
  const header = page.locator('header').filter({ has: page.getByRole('navigation', { name: 'Big screen' }) })
  expect((await header.boundingBox())?.y).toBe(0)
  await expect(header.getByTestId('window-controls-room')).toHaveCount(1)
  await expect(page.getByTestId('window-controls')).toBeVisible()
  const close = (await page.getByRole('button', { name: 'Close' }).boundingBox())!
  const bar = (await header.boundingBox())!
  expect(close.x + close.width).toBe(bar.x + bar.width)
  await page.getByRole('link', { name: 'Library' }).focus()
  for (const key of ['ArrowUp', 'ArrowUp', 'ArrowRight', 'ArrowRight', 'ArrowRight', 'ArrowRight', 'ArrowRight', 'ArrowRight']) await page.keyboard.press(key)
  expect(await page.evaluate(() => document.activeElement?.closest('[data-testid="window-controls"]') == null)).toBe(true)
})

// A window opened with --big-screen, full screen.
test('a window opened in big screen is full screen: no title bar, Close UpNext in the menu, and the browser’s choice kept', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await page.addInitScript(`
    window.sent = []
    window.upnextApp = true
    window.upnextBigScreen = true
    window.upnextFrame = { os: 'linux' }
    window.ipc = { postMessage: (m) => {
      window.sent.push(m)
      // The window answers the title bar as src/desktop.rs does when it's full screen.
      if (m === 'window:framed') setTimeout(() => window.dispatchEvent(new CustomEvent('upnext:window', { detail: { maximized: false, focused: true, fullscreen: true } })))
    } }
    localStorage.setItem('upnext.bigscreen', '0')
  `)
  await page.goto('/library')
  await expect(page.getByRole('navigation', { name: 'Big screen' })).toBeVisible()
  // The window is told the page is in big screen, so it stays full screen.
  await expect.poll(() => page.evaluate<string[]>('window.sent')).toContain('big-screen:on')
  await expect(page.getByTestId('title-bar')).toHaveCount(0)
  await expect(page.getByTestId('window-controls')).toHaveCount(0)
  const header = page.locator('header').filter({ has: page.getByRole('navigation', { name: 'Big screen' }) })
  await expect.poll(async () => (await header.boundingBox())?.y).toBe(0)
  // The window's choice isn't the browser's.
  expect(await page.evaluate(() => localStorage.getItem('upnext.bigscreen'))).toBe('0')

  // With no title bar, the button help closes the window.
  await page.keyboard.press('?')
  await page.getByRole('button', { name: 'Close UpNext' }).click()
  expect(await page.evaluate<string[]>('window.sent')).toContain('window:close')
})

test('the first run with a controller: no typing, then the tour', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(e.message))
  await page.goto('/setup?big=1')
  await expect(page.getByRole('heading', { name: 'Welcome to UpNext' })).toBeVisible()
  // No pages to go to yet.
  await expect(page.getByRole('navigation', { name: 'Big screen' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Start' })).toBeFocused()
  expect(await smallText(page, 14)).toEqual([])

  await page.keyboard.press('Enter')
  await expect(page.getByRole('heading', { name: 'Your libraries' })).toBeVisible()
  // This test's server has no Steam or Heroic folder: nothing to read.
  await expect(page.getByText('Steam: not found')).toBeVisible()
  expect(await smallText(page, 14)).toEqual([])
  await page.getByRole('button', { name: 'Not now' }).focus()
  await page.keyboard.press('Enter')

  await expect(page.getByRole('heading', { name: 'The AI' })).toBeVisible()
  await page.getByRole('button', { name: 'Finish on your phone or computer' }).focus()
  await page.keyboard.press('Enter')
  await expect(page.getByRole('heading', { name: 'Finish on your phone or computer' })).toBeVisible()
  expect(await smallText(page, 14)).toEqual([])
  // Back closes nothing here, and doesn't leave setup.
  await page.keyboard.press('Escape')
  await expect(page).toHaveURL(/\/setup/)
  await page.getByRole('button', { name: 'Back', exact: true }).focus()
  await page.keyboard.press('Enter')
  await page.getByRole('button', { name: /Next|Go on without it/ }).first().focus()
  await page.keyboard.press('Enter')

  await expect(page.getByRole('heading', { name: 'Ready' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Show me around' })).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(page).toHaveURL(/:\d+\/$/)
  const tour = page.getByRole('dialog', { name: 'Say what you’re up for' })
  await expect(tour).toBeVisible()
  expect(await smallText(page, 14)).toEqual([])
  for (let i = 0; i < 3; i++) await page.getByRole('button', { name: 'Next' }).click()
  await page.getByRole('button', { name: 'Done' }).click()
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await expect(page.getByRole('navigation', { name: 'Big screen' })).toBeVisible()
  expect(errors).toEqual([])
})
