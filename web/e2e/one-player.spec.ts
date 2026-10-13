import { expect, test, type Page } from '@playwright/test'

// One player, as most installs are. A second server runs the mock
// library's first player alone (`mock-library --one-player`, started by
// playwright.config.ts). Nothing about players shows: no "Who's playing?",
// no switcher, no "You" in Settings, no Hidden by me; the wizard's "Who
// plays here?" step offers the computer's user name (serve.sh's `sam`) for
// the made-up "Player 1" and adds no one; and a pick works as before.
const port = Number(process.env.UPNEXT_ONE_PLAYER_PORT ?? 8797)
test.use({ baseURL: `http://127.0.0.1:${port}`, storageState: { cookies: [], origins: [] } })

const heading = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true })

test('with one player, setup and picks show nothing about players', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(e.message))
  const players = await (await page.request.get('/api/v1/players')).json()
  expect(players.players).toHaveLength(1)

  // A new browser opens on the library, with the profile link where the
  // switcher would be, and no picker.
  await page.goto('/library')
  await expect(page.getByRole('link', { name: 'Your profile' })).toBeVisible()
  await expect(page.getByRole('dialog', { name: 'Who’s playing?' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: /^Playing as / })).toHaveCount(0)

  // The wizard again, start to end: every step's Next, the taste questions
  // skipped, and at "Who plays here?" the name offered taken and no one added.
  await page.goto('/setup?again=1')
  await page.getByRole('button', { name: 'Start' }).click()
  await expect(heading(page, 'Where UpNext runs')).toBeVisible()
  const taste = heading(page, 'Your taste')
  for (let i = 0; i < 12 && !(await taste.isVisible()); i++) {
    await page.getByRole('button', { name: 'Next', exact: true }).click()
    await page.waitForTimeout(100)
  }
  await expect(taste).toBeVisible()
  await page.getByRole('button', { name: 'Skip this step' }).first().click()
  await expect(heading(page, 'Who plays here?')).toBeVisible()
  await expect(page.getByLabel(/^Your name/)).toHaveValue('Sam')
  await expect(page.getByText('Left empty, you stay Player 1.', { exact: false })).toBeVisible()
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await expect(heading(page, 'You’re set up').or(heading(page, "You're set up"))).toBeVisible()
  await page.getByRole('button', { name: "I'll explore" }).click()
  await expect(page).toHaveURL(/\/library$/)
  const after = (await (await page.request.get('/api/v1/players')).json()).players as { name: string }[]
  expect(after.map((p) => p.name), 'still one player, now named').toEqual(['Sam'])

  // A pick, for the one player.
  await page.goto('/')
  const picked = page.waitForResponse((r) => r.request().method() === 'POST' && new URL(r.url()).pathname === '/api/v1/picks')
  await page.getByRole('button', { name: 'Curate', exact: true }).click()
  expect((await picked).ok()).toBe(true)
  await expect(page.getByRole('region', { name: 'Lineup' }).getByRole('button', { name: /^Open / }).first()).toBeVisible()
  await expect(page.getByRole('dialog', { name: 'Who’s playing?' })).toHaveCount(0)

  // Settings has no "You" group, and Lists no Hidden by me.
  await page.goto('/settings')
  await expect(page.getByRole('heading', { name: 'Players' }).first()).toBeVisible()
  await expect(page.getByText(/^You: /)).toHaveCount(0)
  await page.goto('/lists')
  await expect(page.getByRole('button', { name: /^Play later/ }).first()).toBeVisible()
  await expect(page.getByRole('button', { name: /^Hidden by me/ })).toHaveCount(0)
  expect(errors).toEqual([])
})
