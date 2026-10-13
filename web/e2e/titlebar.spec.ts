import { expect, test } from '@playwright/test'

// The desktop app's title bar. A browser never shows it; the
// app's frameless window says so with `window.upnextFrame`, which these tests
// inject as the window does, with a stand-in for wry's `window.ipc`.

test('a browser shows no title bar, and the page starts at the top', async ({ page }) => {
  await page.goto('/library')
  await expect(page.getByRole('link', { name: 'UpNext home' })).toBeVisible()
  await expect(page.getByTestId('title-bar')).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Minimize' })).toHaveCount(0)
  expect(await page.evaluate<string | null>('document.documentElement.dataset.frame ?? null')).toBeNull()
  const header = await page.locator('header[data-tour="topbar"]').boundingBox()
  expect(header?.y).toBe(0)
})

test('in the frameless window the top bar is the title bar, with the window buttons after Settings', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  // Windows: the page's own scrollbar gives way to the floating one, so the
  // top bar spans the whole window (Chromium here, like WebView2, keeps a
  // strip for the page's scrollbar).
  await page.addInitScript(`
    window.sent = []
    window.upnextApp = true
    window.upnextFrame = { os: 'windows' }
    window.ipc = { postMessage: (m) => window.sent.push(m) }
  `)
  await page.goto('/library')
  const sent = () => page.evaluate<string[]>('window.sent')
  await expect.poll(sent).toContain('window:framed')

  // One bar: no row above the top bar, and the app's name once.
  await expect(page.getByTestId('title-bar')).toHaveCount(0)
  const header = page.locator('header[data-tour="topbar"]')
  expect(await header.boundingBox()).toMatchObject({ x: 0, y: 0, width: 1440, height: 64 })
  await expect(page.getByText('UpNext', { exact: true })).toHaveCount(1)
  const controls = page.getByTestId('window-controls')
  await expect(controls).toBeVisible()
  // Minimize, maximize and close come after Settings, close in the corner.
  const settings = (await header.getByRole('link', { name: 'Settings' }).boundingBox())!
  const minimize = (await page.getByRole('button', { name: 'Minimize' }).boundingBox())!
  expect(minimize.x).toBeGreaterThanOrEqual(settings.x + settings.width)
  const close = (await page.getByRole('button', { name: 'Close' }).boundingBox())!
  expect(close.x + close.width).toBe(1440)
  expect(close.y).toBe(0)

  // The top bar still sticks to the top when the page scrolls.
  await page.mouse.wheel(0, 1500)
  await expect.poll(() => page.evaluate<number>('window.scrollY')).toBeGreaterThan(0)
  expect((await header.boundingBox())?.y).toBe(0)
  // The floating scrollbar shows while the page scrolls, below the window's buttons.
  const scrollbar = page.getByTestId('page-scrollbar')
  await expect(scrollbar).toHaveCSS('opacity', '1')
  expect((await scrollbar.boundingBox())?.y).toBeGreaterThanOrEqual(close.y + close.height)

  // The bar's empty room moves the window, and a double click maximizes;
  // its tabs and links don't.
  const tabs = (await header.getByRole('navigation', { name: 'Main' }).boundingBox())!
  const room = { x: tabs.x + tabs.width + 40, y: 32 }
  await page.mouse.move(room.x, room.y)
  await page.mouse.down()
  await page.mouse.move(room.x + 20, room.y + 5)
  await page.mouse.up()
  expect(await sent()).toContain('window:drag')
  await page.mouse.dblclick(room.x, room.y)
  expect(await sent()).toContain('window:maximize')
  await page.evaluate('window.sent = []')
  const library = (await header.getByRole('link', { name: /Library/ }).boundingBox())!
  await page.mouse.move(library.x + 10, library.y + 10)
  await page.mouse.down()
  await page.mouse.move(library.x + 40, library.y + 20)
  await page.mouse.up()
  expect(await sent()).toEqual([])

  // The window's buttons, and what the window says back.
  await page.getByRole('button', { name: 'Maximize' }).click()
  expect(await sent()).toContain('window:maximize')
  await page.evaluate(`window.dispatchEvent(new CustomEvent('upnext:window', { detail: { maximized: true, focused: true } }))`)
  await expect(page.getByRole('button', { name: 'Restore' })).toBeVisible()
  await expect(page.locator('[data-edge]')).toHaveCount(0)
  await page.getByRole('button', { name: 'Close' }).click()
  expect(await sent()).toContain('window:close')

  // Full screen: no window buttons, and the top bar keeps the top.
  await page.evaluate(`window.dispatchEvent(new CustomEvent('upnext:window', { detail: { maximized: false, focused: true, fullscreen: true } }))`)
  await expect(page.getByTestId('window-controls')).toHaveCount(0)
  await expect(page.getByTestId('title-bar')).toHaveCount(0)
  expect((await header.boundingBox())?.y).toBe(0)

  // A game's sheet opens below the top bar, not under it.
  await page.goto('/library')
  await page.getByRole('button', { name: /^Stardew Valley/ }).click()
  const sheet = page.getByRole('dialog', { name: 'Stardew Valley' })
  await expect(sheet).toBeVisible()
  expect((await sheet.boundingBox())!.y).toBeGreaterThanOrEqual(64)
})

// The top bar with the window's buttons still fits the window at the widths
// the app window goes to, with nothing pushed off its end. (Linux: here
// Chromium keeps the page's scrollbar beside the page, as Windows' doesn't.)
test('the top bar fits with the window buttons from the smallest window up', async ({ page }) => {
  await page.addInitScript(`
    window.upnextApp = true
    window.upnextFrame = { os: 'linux' }
    window.ipc = { postMessage: () => {} }
  `)
  for (const width of [640, 800, 1024, 1280, 1366, 1536, 1760, 1920]) {
    await page.setViewportSize({ width, height: 800 })
    await page.goto('/library')
    const header = page.getByRole('banner')
    const bar = (await header.boundingBox())!
    const close = (await page.getByRole('button', { name: 'Close' }).boundingBox())!
    expect(close.x + close.width, `${width} px`).toBe(bar.x + bar.width)
    const settings = (await header.getByRole('link', { name: 'Settings' }).boundingBox())!
    const minimize = (await page.getByRole('button', { name: 'Minimize' }).boundingBox())!
    expect(minimize.x, `${width} px`).toBeGreaterThanOrEqual(settings.x + settings.width)
    expect(await header.evaluate((h) => h.scrollWidth <= h.clientWidth), `${width} px`).toBe(true)
    // From 1024 px the tabs show whole and the search box keeps room to type.
    if (width >= 1024) {
      const nav = header.getByRole('navigation', { name: 'Main' })
      expect(await nav.evaluate((n) => n.scrollWidth <= n.clientWidth), `${width} px`).toBe(true)
      expect((await header.locator('input').boundingBox())!.width, `${width} px`).toBeGreaterThan(80)
    }
    const sync = header.getByRole('button', { name: /^Synced|^Not synced/ })
    if (await sync.isVisible()) {
      const clipped = await sync.evaluate((b) => [...b.querySelectorAll('span')].some((s) => s.scrollWidth > s.clientWidth + 1))
      expect(clipped, `sync status clipped at ${width} px`).toBe(false)
    }
  }
})

// A page with no top bar (the first run's setup) gets a slim bar with the
// window's buttons, and nothing else.
test('setup in the frameless window has a slim bar with the window buttons', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await page.addInitScript(`
    window.upnextApp = true
    window.upnextFrame = { os: 'linux' }
    window.ipc = { postMessage: () => {} }
  `)
  await page.goto('/setup')
  const bar = page.getByTestId('title-bar')
  await expect(bar).toBeVisible()
  const box = (await bar.boundingBox())!
  expect(box).toMatchObject({ x: 0, y: 0, height: 32 })
  const close = (await page.getByRole('button', { name: 'Close' }).boundingBox())!
  expect(close.x + close.width).toBe(box.width)
  await expect(bar).toHaveText('')
  await expect(page.getByRole('button', { name: 'Close' })).toHaveCount(1)
})

// The window's buttons stay above every panel, dialog and the tour, as the
// title bar did, so the window can always be closed or moved off.
test('the window buttons stay reachable over the tour and Activity', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.addInitScript(`
    window.sent = []
    window.upnextApp = true
    window.upnextFrame = { os: 'windows' }
    window.ipc = { postMessage: (m) => window.sent.push(m) }
  `)
  const sent = () => page.evaluate<string[]>('window.sent')
  await page.goto('/settings')
  await page.getByRole('link', { name: 'Show me around' }).click()
  await expect(page.getByRole('dialog').first()).toBeVisible()
  // Playwright clicks only what the pointer would reach at that point.
  await page.getByRole('button', { name: 'Close' }).click()
  expect(await sent()).toContain('window:close')
  await page.getByRole('button', { name: 'Minimize' }).click()
  expect(await sent()).toContain('window:minimize')
  await page.keyboard.press('Escape')

  await page.goto('/library')
  await page.getByRole('banner').getByRole('button', { name: /^Synced|^Not synced/ }).click()
  await expect(page.getByRole('dialog', { name: /Activity/ })).toBeVisible()
  await page.evaluate('window.sent = []')
  await page.getByRole('button', { name: 'Maximize' }).click()
  expect(await sent()).toContain('window:maximize')
})
