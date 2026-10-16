import { expect, test } from '@playwright/test'

// One pass through the app on the mock library: ask for picks (the fake
// model answers), open a picked game's sheet, and mark it as beaten.
test('pick, open a game, mark it beaten', async ({ page, request }) => {
  await page.goto('/')
  await page.getByLabel('What are you in the mood for?').fill('A quiet hour')
  await page.getByRole('button', { name: 'Curate', exact: true }).click()

  const lineup = page.getByRole('region', { name: 'Lineup' })
  const opens = lineup.getByRole('button', { name: /^Open / })
  await expect(opens).toHaveCount(3)
  // Each card shows the model's reason for that pick.
  const [latest] = await (await request.get('/api/v1/picks/history?limit=1')).json()
  await expect(lineup.getByText(latest.picks[0].reason).first()).toBeVisible()

  // The first pick with an ending, so "Beat it" is on its menu.
  let marked: string | null = null
  for (const open of await opens.all()) {
    const name = (await open.getAttribute('aria-label'))!.replace(/^Open /, '')
    await open.click()
    const sheet = page.getByRole('dialog', { name })
    await expect(sheet).toBeVisible()
    await sheet.getByRole('button', { name: /Mark as/ }).click()
    const beat = sheet.getByRole('menuitemradio', { name: /Beat it/ })
    if (await beat.count()) {
      await beat.click()
      await expect(sheet.getByRole('button', { name: 'Change' })).toBeVisible()
      await expect(sheet.getByText('since today')).toBeVisible()
      marked = name
      break
    }
    await page.keyboard.press('Escape')
    await page.keyboard.press('Escape')
    await expect(sheet).toBeHidden()
  }
  expect(marked, 'one of the picks has an ending').not.toBeNull()

  // The server kept it.
  const found = await request.get(`/api/v1/games?status=done&q=${encodeURIComponent(marked!)}`)
  const body = await found.json()
  expect(body.items.map((g: { name: string }) => g.name)).toContain(marked)
})

// Every page opens from its address and from the tabs, with no script
// errors (most of them load on first use).
test('every page opens', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(e.message))
  for (const path of ['/library', '/collections', '/history', '/lists', '/settings', '/profile', '/profile/questions', '/versus']) {
    await page.goto(path)
    await expect(page.locator('main'), path).not.toBeEmpty()
    await expect(page.locator('main'), path).toContainText(/\w{3}/)
  }
  await page.goto('/')
  await page.getByRole('link', { name: 'Settings', exact: true }).click()
  await expect(page).toHaveURL(/\/settings$/)
  await expect(page.locator('main')).toContainText(/\w{3}/)
  expect(errors).toEqual([])
})

// A tab open across an upgrade asks for its old build's scripts, which the
// new build doesn't have (code health B-WEB-1): it loads itself again once,
// and when the script is still gone it says so, with Reload, rather than
// going blank.
test('a page open across an upgrade loads again, and says why when that isn’t enough', async ({ page }) => {
  const settingsScript = '**/assets/SettingsPage-*.js'
  let asked = 0
  // The old build's name for Settings' script, which the server doesn't have.
  await page.route(settingsScript, async (route) => {
    asked++
    await route.continue({ url: new URL('/assets/SettingsPage-0ld0ld00.js', route.request().url()).href })
  })
  let loads = 0
  page.on('load', () => loads++)
  await page.goto('/')
  await page.getByRole('link', { name: 'Settings', exact: true }).click()
  const alert = page.getByRole('alert').filter({ hasText: 'UpNext was updated while this page was open' })
  await expect(alert).toBeVisible()
  expect(asked, 'asked again after the reload').toBeGreaterThanOrEqual(2)
  expect(loads, 'one reload, not a loop').toBe(2)
  await expect(page).toHaveURL(/\/settings$/)

  // The new build's script is there: Reload opens Settings.
  await page.unroute(settingsScript)
  await alert.getByRole('button', { name: 'Reload' }).click()
  await expect(page.locator('main')).toContainText(/\w{3}/)
  await expect(alert).toBeHidden()
})

// Below 1024 px the top bar has no search box, so the Library has its own,
// and / reaches it from any page; from 1024 px / finds the top bar's.
test('search the library by name at any width', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/collections')
  await expect(page.locator('main')).toContainText(/\w{3}/)
  await page.keyboard.press('/')
  await expect(page).toHaveURL(/\/library$/)
  const box = page.getByRole('searchbox', { name: 'Search your library' })
  await expect(box).toBeFocused()

  const tiles = page.getByRole('region', { name: 'Games' }).getByRole('listitem')
  await expect(tiles.nth(1)).toBeVisible()
  const before = await tiles.count()
  const name = (await tiles.nth(1).locator('.font-semibold').innerText()).trim()
  await box.fill(name)
  await expect(page).toHaveURL(/[?&]q=/)
  await expect(tiles).not.toHaveCount(before)
  await expect(tiles.first()).toContainText(name)

  await page.setViewportSize({ width: 1280, height: 800 })
  await expect(page.getByRole('searchbox')).toHaveCount(1)
  const top = page.getByRole('banner').getByRole('searchbox', { name: 'Search your library' })
  await expect(top).toHaveValue(name)
  await page.locator('main').click({ position: { x: 5, y: 5 } })
  await page.keyboard.press('/')
  await expect(top).toBeFocused()
})

// The outermost element past the screen's right edge that no scrolling box
// holds, to say what widened the page, or null when the page fits.
const widener = `(() => {
  if (document.documentElement.scrollWidth <= innerWidth) return null
  for (const el of document.querySelectorAll('body *')) {
    if (el.getBoundingClientRect().right <= innerWidth + 1) continue
    let held = false
    for (let a = el.parentElement; a && a !== document.body; a = a.parentElement) if (getComputedStyle(a).overflowX !== 'visible') held = true
    if (!held) return el.outerHTML.slice(0, 200)
  }
  return 'the page is ' + document.documentElement.scrollWidth + ' px wide'
})()`

const PAGES = ['/', '/library', '/library?view=table', '/collections', '/history', '/lists', '/settings', '/profile', '/versus']

// At a phone's width no page is wider than the screen: the top bar's tabs
// fit, and wide tables (the library's, Jobs') scroll in their own box.
test('every page fits a phone', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  for (const path of PAGES) {
    await page.goto(path)
    await expect(page.locator('main'), path).toContainText(/\w{3}/)
    expect(await page.evaluate<string | null>(widener), path).toBeNull()
  }
})

// On a phone the request box sits above Curate, at its full height, not
// squeezed to its text.
test('the request box keeps its height on a phone', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/')
  expect((await page.getByLabel('What are you in the mood for?').boundingBox())?.height).toBe(60)
})

// Nor at a tablet's or a small window's width, where the top bar gains the
// wordmark, the sync status and then the search box: at each width where
// one of them appears, and between, every page still fits.
test('every page fits a tablet and a small window', async ({ page }) => {
  // Nine pages at seven widths are 63 page loads, which took most of the
  // default 30 seconds on GitHub's runner and ran out on a slower
  // self-hosted one, so this test gets three times the limit.
  test.slow()
  for (const width of [640, 700, 768, 800, 1024, 1100, 1180]) {
    await page.setViewportSize({ width, height: 800 })
    for (const path of PAGES) {
      await page.goto(path)
      await expect(page.locator('main'), path).toContainText(/\w{3}/)
      expect(await page.evaluate<string | null>(widener), `${path} at ${width} px`).toBeNull()
    }
  }
})

// The ask box stops at the 2,000 characters the server takes, and a long
// request with no spaces wraps on History rather than widen the page.
test('a long request fits History', async ({ page }) => {
  await page.goto('/')
  const ask = page.getByLabel('What are you in the mood for?')
  await ask.fill('x'.repeat(2500))
  await expect(ask).toHaveValue('x'.repeat(2000))
  // An earlier test's lineup may be on screen, so wait for this request's.
  const picked = page.waitForResponse((r) => r.request().method() === 'POST' && new URL(r.url()).pathname === '/api/v1/picks')
  await page.getByRole('button', { name: 'Curate', exact: true }).click()
  expect((await picked).ok()).toBe(true)
  for (const width of [390, 1280]) {
    await page.setViewportSize({ width, height: 800 })
    await page.goto('/history')
    await expect(page.locator('main')).toContainText('x'.repeat(100))
    expect(await page.evaluate<string | null>(widener), `/history at ${width} px`).toBeNull()
  }
})

// On a wide window Versus keeps the two store headers near their own size
// rather than blowing them up to half the window each.
test('Versus covers stay near their size on a wide window', async ({ page, request }) => {
  const games = await (await request.get('/api/v1/games?limit=2')).json()
  const [a, b] = games.items.map((g: { id: number }) => g.id)
  await page.setViewportSize({ width: 1920, height: 1080 })
  await page.goto(`/versus?a=${a}&b=${b}`)
  const covers = page.getByRole('region', { name: 'The two games' }).getByRole('button', { name: /^Open / })
  await expect(covers).toHaveCount(2)
  for (const cover of await covers.all()) {
    expect((await cover.boundingBox())!.width).toBeLessThanOrEqual(520)
  }
})

// The profile opens with its heading and actions, and on a wide window
// lays its sections side by side rather than in one long column.
test('the profile uses a wide window', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/profile')
  // The mock library has two players, so the profile says whose it is.
  await expect(page.getByRole('heading', { level: 1, name: 'Player 1’s profile' })).toBeVisible()
  await expect(page.getByRole('link', { name: 'Tune your profile' })).toBeVisible()
  await expect(page.getByRole('link', { name: 'Export as JSON' })).toBeVisible()
  const patterns = (await page.locator('#patterns').boundingBox())!
  const habits = (await page.locator('#habits').boundingBox())!
  expect(habits.x).toBeGreaterThan(patterns.x + patterns.width - 1)
  expect(Math.abs(habits.y - patterns.y)).toBeLessThan(2)
})

// The top bar with a Steam name and picture: the sync status is the whole
// line or only its dot, never clipped, and the picture keeps its size.
// The Steam profile is in the top bar with one player, so this test has one.
test('the top bar fits with a profile name and picture', async ({ page }) => {
  await page.route('**/api/v1/players', async (route) => {
    const body = await (await route.fetch()).json()
    await route.fulfill({ json: { ...body, players: body.players.slice(0, 1) } })
  })
  await page.route('**/api/v1/profile', async (route) => {
    const body = await (await route.fetch()).json()
    body.persona_name = 'A rather long persona'
    body.avatar = 'data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" width="184" height="100"/>'
    await route.fulfill({ json: body })
  })
  for (const width of [1280, 1366, 1536, 1760, 1920]) {
    await page.setViewportSize({ width, height: 800 })
    await page.goto('/library')
    const sync = page.getByRole('banner').getByRole('button', { name: /^Synced|^Not synced/ })
    await expect(sync, `${width} px`).toBeVisible()
    const clipped = await sync.evaluate((b) => [...b.querySelectorAll('span')].some((s) => s.scrollWidth > s.clientWidth + 1))
    expect(clipped, `sync status clipped at ${width} px`).toBe(false)
    const avatar = page.getByRole('link', { name: /^Your profile/ }).locator('img')
    expect(await avatar.boundingBox(), `${width} px`).toMatchObject({ width: 24, height: 24 })
    expect(await page.evaluate<string | null>(widener), `${width} px`).toBeNull()
  }
})

// With two players the top bar has "Playing as" in place of the Steam
// profile, and still fits with the sync status unclipped.
test('the top bar fits the player menu', async ({ page }) => {
  for (const width of [1280, 1366, 1536, 1760, 1920]) {
    await page.setViewportSize({ width, height: 800 })
    await page.goto('/library')
    await expect(page.getByRole('banner').getByRole('button', { name: 'Playing as Player 1' }), `${width} px`).toBeVisible()
    const sync = page.getByRole('banner').getByRole('button', { name: /^Synced|^Not synced/ })
    const clipped = await sync.evaluate((b) => [...b.querySelectorAll('span')].some((s) => s.scrollWidth > s.clientWidth + 1))
    expect(clipped, `sync status clipped at ${width} px`).toBe(false)
    expect(await page.evaluate<string | null>(widener), `${width} px`).toBeNull()
  }
})

// Settings fits a 320 px phone whichever AI is chosen, though each shows
// its own fields: a server's provider and address, or a key and models.
test('Settings fits a small phone with each AI', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 800 })
  for (const backend of ['openai', 'api-key', 'openai-key', 'codex-cli']) {
    await page.unrouteAll()
    await page.route('**/api/v1/settings', async (route) => {
      if (route.request().method() !== 'GET') return route.continue()
      const body = await (await route.fetch()).json()
      body.llm_backend = backend
      await route.fulfill({ json: body })
    })
    await page.goto('/settings')
    await expect(page.locator('#picks')).toBeVisible()
    await page.waitForLoadState('networkidle')
    expect(await page.evaluate<string | null>(widener), backend).toBeNull()
  }
})

// Settings › Accounts at the widths of the Windows retest, where Steam's
// "Found on this computer" ran over the Steam name and left its description
// no room: with a Steam account found on this computer and names as long as
// Steam, GOG and Epic allow, no row's right side crosses its name, and
// Steam's description shows whole. And at a phone's widths, where the first
// fix kept a long row name ("Backlog verdicts from other players") from
// wrapping, Settings is no wider than the screen.
test('the account rows keep their names clear at desktop widths', async ({ page }) => {
  const long = 'Sam the very patient backlogger'
  const fake = async (path: string, change: object) =>
    page.route(`**/api/v1/${path}`, async (route) => {
      if (route.request().method() !== 'GET') return route.continue()
      const body = await (await route.fetch()).json()
      await route.fulfill({ json: { ...body, ...change } })
    })
  await fake('settings', { steam_id: '76561197960287930', steam_id_source: 'steam client', steam_persona: long, steam_api_key_set: true })
  await fake('gog', { connected: true, username: long })
  await fake('epic', { connected: true, display_name: long })
  for (const width of [320, 390, 1024, 1280, 1536, 1920]) {
    await page.setViewportSize({ width, height: 900 })
    await page.goto('/settings')
    const accounts = page.locator('#accounts')
    await expect(accounts.getByText(`Found on this computer: ${long}`)).toBeVisible()
    await expect(accounts.getByText(long, { exact: true })).toHaveCount(2)
    const crossed = await accounts.evaluate((section) => {
      const bad: string[] = []
      // Each row's first line holds the name's side, then the right side.
      for (const line of section.querySelectorAll<HTMLElement>('.min-h-11')) {
        const [left, right] = line.children
        const name = left.querySelector('span')!.getBoundingClientRect()
        if (!right) continue
        const r = right.getBoundingClientRect()
        if (name.right > r.left && name.bottom > r.top) bad.push(left.textContent!.slice(0, 40))
      }
      return bad
    })
    expect(crossed, `${width} px`).toEqual([])
    const desc = accounts.getByText('Library, playtime and installs', { exact: true })
    expect(await desc.evaluate((el) => el.scrollWidth <= el.clientWidth), `${width} px`).toBe(true)
    await expect(page.getByText(/^Backlog verdicts from other players/).first()).toBeVisible()
    expect(await page.evaluate<string | null>(widener), `${width} px`).toBeNull()
  }
})

// Ratings: a pick's thumbs, with the why after Not a fit, leave
// the lineup as it is and are kept; a game you've given a status can be
// rated on its sheet.
test('rate a pick, then the game', async ({ page, request }) => {
  await page.goto('/')
  await page.getByLabel('What are you in the mood for?').fill('Something to rate')
  await page.getByRole('button', { name: 'Curate', exact: true }).click()
  const lineup = page.getByRole('region', { name: 'Lineup' })
  const cards = lineup.getByRole('article')
  await expect(cards).toHaveCount(3)
  const card = cards.first()
  const name = (await card.getByRole('button', { name: /^Open / }).getAttribute('aria-label'))!.replace(/^Open /, '')

  await card.getByRole('button', { name: 'Not a fit' }).click()
  await card.getByRole('button', { name: 'Wrong mood' }).click()
  await expect(card.getByRole('button', { name: 'Wrong mood' })).toHaveAttribute('aria-pressed', 'true')
  await expect(card.getByRole('button', { name: 'Not a fit' })).toHaveAttribute('aria-pressed', 'true')
  await expect(cards).toHaveCount(3)
  await expect(cards.first().getByRole('heading')).toHaveText(name)
  const history = await (await request.get('/api/v1/picks/history?limit=1')).json()
  expect(history[0].picks[0].rating).toMatchObject({ verdict: 'not_a_fit', reason: 'wrong_mood' })

  await card.getByRole('button', { name: `Open ${name}` }).click()
  const sheet = page.getByRole('dialog', { name })
  await expect(sheet).toBeVisible()
  await sheet.getByRole('button', { name: /Mark as/ }).click()
  await sheet.getByRole('menuitemradio').first().click()
  // A status that asks why it stopped can skip the question.
  const skip = sheet.getByRole('button', { name: /^Skip/ })
  if (await skip.count()) await skip.click()
  const stars = sheet.getByRole('group', { name: 'Your rating' })
  await expect(stars).toBeVisible()
  await stars.getByRole('button', { name: '4 stars: Liked it' }).click()
  await expect(stars.getByRole('button', { name: '4 stars: Liked it' })).toHaveAttribute('aria-pressed', 'true')
  const id = history[0].picks[0].id
  const game = await (await request.get(`/api/v1/games/${id}`)).json()
  expect(game.my_rating).toBe(4)
  await page.keyboard.press('Escape')

  // History rates picks too, the Profile counts both ratings,
  // and the Library filters by yours.
  await page.goto('/history')
  await expect(page.getByText(/· 1 not a fit/).first()).toBeVisible()
  // Exact, since a lineup's own button says how its picks were rated.
  const good = page.getByRole('button', { name: 'Good pick', exact: true }).nth(1)
  await good.click()
  await expect(good).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByText(/· 1 good pick, 1 not a fit/).first()).toBeVisible()

  await page.goto('/profile')
  await expect(page.getByText(/Of the 2 picks you rated, 1 was a good pick\./)).toBeVisible()
  await expect(page.getByText(/1 game: 1 “Liked it”\./)).toBeVisible()

  await page.goto('/library?view=table&rating=rated')
  await expect(page.getByRole('combobox', { name: 'Your rating' })).toHaveValue('rated')
  await expect(page.getByRole('row').filter({ hasText: name })).toContainText('★ 4')
})

// Roll the dice: one game off your radar, by chance and with no AI,
// with Another and Curate around this on its card; and a pick's art says
// it opens the game and previews its screenshots on hover.
test('roll the dice, then curate around it; a pick previews its screenshots', async ({ page, request }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Roll the dice' }).click()
  const lineup = page.getByRole('region', { name: 'Lineup' })
  await expect(lineup.getByRole('heading', { name: 'Rolled the dice' })).toBeVisible()
  const cards = lineup.getByRole('article')
  await expect(cards).toHaveCount(1)
  await expect(cards.first()).toContainText('Drawn at random')
  await expect(cards.first().getByLabel('Compare')).toHaveCount(0)
  await expect(lineup).toContainText(/drawn from \d+ games off your radar · no AI call/)
  const first = (await cards.first().getByRole('heading').textContent())!
  let history = await (await request.get('/api/v1/picks/history?limit=1')).json()
  expect(history[0]).toMatchObject({ mode: 'random', model: null, attempts: 0, shown: 1 })

  await cards.first().getByRole('button', { name: 'Another' }).click()
  await expect(cards.first().getByRole('heading')).not.toHaveText(first)
  await expect(cards).toHaveCount(1)
  const second = (await cards.first().getByRole('heading').textContent())!

  await cards.first().getByRole('button', { name: 'Curate around this' }).click()
  await expect(page.getByLabel('What are you in the mood for?')).toHaveValue(`Games like ${second}`)
  await expect(cards).toHaveCount(3)
  history = await (await request.get('/api/v1/picks/history?limit=1')).json()
  await expect(lineup.getByText(history[0].picks[0].reason).first()).toBeVisible()
  expect(history[0].prompt).toBe(`Games like ${second}`)
  expect(history[0].picks.map((p: { name: string }) => p.name)).not.toContain(second)

  // The art points, and resting the mouse on it fades through the game's
  // screenshots inside the art's own frame. The mock library has no art,
  // so the game gets three made-up screenshots here.
  const art = cards.first().getByRole('button', { name: /^Open / })
  const name = (await art.getAttribute('aria-label'))!.replace(/^Open /, '')
  await expect(art).toHaveCSS('cursor', 'pointer')
  const tints = ['#2a6f97', '#b5651d', '#4f772d']
  await page.route('**/e2e-shots/*.svg', (route) => {
    const n = Number(route.request().url().match(/(\d)\.svg$/)![1])
    return route.fulfill({
      contentType: 'image/svg+xml',
      body: `<svg xmlns="http://www.w3.org/2000/svg" width="600" height="338"><rect width="600" height="338" fill="${tints[n]}"/></svg>`,
    })
  })
  await page.route(/\/api\/v1\/games\/\d+$/, async (route) => {
    const response = await route.fetch()
    const shots = tints.map((_, n) => ({ thumb: `/e2e-shots/${n}.svg`, full: `/e2e-shots/${n}.svg` }))
    await route.fulfill({ response, json: { ...(await response.json()), screenshots: shots } })
  })
  const save = cards.first().getByRole('button', { name: /^(Save for later|Saved)$/ })
  const title = cards.first().getByRole('heading', { name })
  const shot = (n: number) => art.locator(`img[data-shot="${n}"]`)
  await art.hover()
  const dots = cards.first().getByRole('group', { name: 'Screenshots' })
  await expect(dots).toBeVisible()
  await expect(dots.getByRole('button')).toHaveCount(3)
  await expect(shot(0)).toHaveCSS('opacity', '1')
  await expect(shot(1)).toHaveCSS('opacity', '1', { timeout: 6000 })
  await expect(shot(0)).toHaveCSS('opacity', '0')
  // Nothing grew out of the frame: the title still starts below the art,
  // and a click on the button under it lands on that button.
  const frame = (await art.boundingBox())!
  expect((await title.boundingBox())!.y).toBeGreaterThanOrEqual(frame.y + frame.height)
  const box = (await save.boundingBox())!
  expect(
    await page.evaluate(([x, y]) => document.elementFromPoint(x, y)?.closest('button')?.textContent, [box.x + box.width / 2, box.y + box.height / 2]),
  ).toMatch(/Save for later|Saved/)
  // A dot shows its screenshot and keeps it there.
  await dots.getByRole('button', { name: 'Screenshot 3 of 3' }).hover()
  await expect(shot(2)).toHaveCSS('opacity', '1')
  await page.waitForTimeout(3000)
  await expect(shot(2)).toHaveCSS('opacity', '1')
  // Moving away ends it, with nothing to close.
  await page.mouse.move(0, 0)
  await expect(dots).toBeHidden()
  await expect(shot(2)).toHaveCSS('opacity', '0')
  await art.click()
  await expect(page.getByRole('dialog', { name })).toBeVisible()
  await page.unroute(/\/api\/v1\/games\/\d+$/)
})
