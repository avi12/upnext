import { expect, test, type Page } from '@playwright/test'

// Two players on the mock library, the first ("Player 1")
// and Sam, each with a Steam account of their own, shared with everyone.
// The first hid Hotline Miami from everyone else. Each sees their own hours
// and the shared games, a pick for each, a mark that stays with whoever made
// it, and a new player's own setup before anything else.
test.use({ storageState: { cookies: [], origins: [] } })

interface Game {
  id: number
  name: string
  playtime_minutes: number
  override: string | null
}

/** A game as the browser's player sees it, or as `player` does. */
async function game(page: Page, name: string, player?: number): Promise<Game | undefined> {
  const headers = player ? { 'UpNext-Player': String(player) } : undefined
  const r = await page.request.get(`/api/v1/games?q=${encodeURIComponent(name)}`, { headers })
  expect(r.ok()).toBe(true)
  return ((await r.json()).items as Game[]).find((g) => g.name === name)
}

async function curate(page: Page) {
  await page.goto('/')
  const picked = page.waitForResponse((r) => r.request().method() === 'POST' && new URL(r.url()).pathname === '/api/v1/picks')
  await page.getByRole('button', { name: 'Curate', exact: true }).click()
  expect((await picked).ok()).toBe(true)
  await expect(page.getByRole('region', { name: 'Lineup' }).getByRole('button', { name: /^Open / }).first()).toBeVisible()
}

test('two players each see their own hours and the shared games, get picks, and keep their own marks', async ({ page }) => {
  await page.goto('/library')
  const picker = page.getByRole('dialog', { name: 'Who’s playing?' })
  await picker.getByRole('button', { name: 'Sam', exact: true }).click()
  await expect(page.getByRole('button', { name: 'Playing as Sam' })).toBeVisible()

  // Sam's hours on the game both own, a game only the first player owns
  // (shared, so there, without hours), and nothing the first player hid.
  expect((await game(page, 'Portal 2'))?.playtime_minutes).toBe(900)
  expect((await game(page, 'Hotline Miami'))).toBeUndefined()
  expect((await game(page, 'Portal 2', 1))?.playtime_minutes).toBe(600)
  expect((await game(page, 'Hotline Miami', 1))?.name).toBe('Hotline Miami')

  // A pick for Sam, and a mark of Sam's.
  await curate(page)
  const stardew = (await game(page, 'Stardew Valley'))!
  const marked = await page.request.put(`/api/v1/games/${stardew.id}/override`, { data: { status: 'abandoned' } })
  expect(marked.ok()).toBe(true)
  expect((await game(page, 'Stardew Valley'))?.override).toBe('abandoned')

  // Switch in one click: the first player's library, without Sam's mark.
  await page.getByRole('button', { name: 'Playing as Sam' }).click()
  await page.getByRole('menuitem', { name: 'Play as Player 1' }).click()
  await expect(page.getByRole('button', { name: 'Playing as Player 1' })).toBeVisible()
  expect((await game(page, 'Stardew Valley'))?.override).not.toBe('abandoned')
  expect((await game(page, 'Hotline Miami'))?.name).toBe('Hotline Miami')
  const history = await page.request.get('/api/v1/picks/history?limit=50')
  const sams = await page.request.get('/api/v1/picks/history?limit=1', { headers: { 'UpNext-Player': '2' } })
  const latest = ((await sams.json()) as { batch_id: string }[])[0]?.batch_id
  expect(latest, "Sam's lineup is in Sam's history").toBeTruthy()
  expect(((await history.json()) as { batch_id: string }[]).map((b) => b.batch_id)).not.toContain(latest)
})

// What one player typed or steered on a page stays theirs: switching gives
// the page afresh to the next player (code health B-WEB-2), where the typed
// request and "Not these, because…" went on to the other player's picks.
test('switching player leaves the last player’s request behind', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('dialog', { name: 'Who’s playing?' }).getByRole('button', { name: 'Player 1', exact: true }).click()
  await expect(page.getByRole('button', { name: 'Playing as Player 1' })).toBeVisible()
  const request = page.getByLabel('What are you in the mood for?')
  await request.fill('Something only Player 1 wants')

  await page.getByRole('button', { name: 'Playing as Player 1' }).click()
  await page.getByRole('menuitem', { name: 'Play as Sam' }).click()
  await expect(page.getByRole('button', { name: 'Playing as Sam' })).toBeVisible()
  await expect(request).toBeVisible()
  await expect(request).not.toHaveValue('Something only Player 1 wants')
})

test("a new player goes through their own setup first, then has their own tour and picks", async ({ page }) => {
  const sam = async () => (await (await page.request.get('/api/v1/setup/state', { headers: { 'UpNext-Player': '2' } })).json()).tour
  const samsTour = await sam()

  await page.goto('/')
  const picker = page.getByRole('dialog', { name: 'Who’s playing?' })
  await picker.getByRole('button', { name: 'Add a player' }).click()
  await picker.getByLabel('Their name').fill('Kit')
  await picker.getByRole('button', { name: 'Add', exact: true }).click()

  await expect(page).toHaveURL(/\/setup\/player$/)
  await expect(page.getByRole('heading', { name: 'Hi, Kit' })).toBeVisible()
  await page.getByRole('button', { name: 'Next' }).click()

  // What the others offer, each on.
  await expect(page.getByRole('heading', { name: 'Your games' })).toBeVisible()
  const uses = page.getByRole('checkbox', { name: 'Use these games' })
  await expect(uses.first()).toBeVisible()
  for (const box of await uses.all()) await expect(box).toBeChecked()
  await page.getByRole('button', { name: 'Next' }).click()

  // The first player's AI, when it's offered.
  await expect(page.getByRole('heading', { name: 'Your AI' })).toBeVisible()
  const useAi = page.getByRole('button', { name: 'Use it for your picks' })
  if (await useAi.count()) {
    await useAi.first().click()
    await expect(page.getByText('In use')).toBeVisible()
  }
  await page.getByRole('button', { name: 'Next' }).click()

  await expect(page.getByRole('heading', { name: 'Your taste' })).toBeVisible()
  await page.getByRole('button', { name: 'Skip this step' }).first().click()
  await expect(page.getByRole('heading', { name: "You're set up, Kit" })).toBeVisible()
  await page.getByRole('button', { name: "I'll explore" }).click()
  await expect(page).toHaveURL(/\/library$/)
  await expect(page.getByRole('button', { name: 'Playing as Kit' })).toBeVisible()

  // Set up, with Kit's tour turned down and Sam's as it was.
  const state = await (await page.request.get('/api/v1/setup/state')).json()
  expect(state).toMatchObject({ player_needed: false, player_setup: 'done', tour: 'skipped', tour_offer: false })
  expect(await sam()).toBe(samsTour)

  // Kit's library is what the others share, and a pick of Kit's own.
  expect((await game(page, 'Portal 2'))?.playtime_minutes).toBe(0)
  await curate(page)

  // Settings › Setup runs Kit's own setup again.
  await page.goto('/settings#setup')
  await page.getByRole('button', { name: 'Run your setup again' }).click()
  await expect(page.getByRole('heading', { name: 'Hi, Kit' })).toBeVisible()
  await page.getByRole('button', { name: 'Skip setup' }).click()
  await expect(page).toHaveURL(/\/$/)
})
