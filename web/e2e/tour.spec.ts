import { expect, test, type Page } from '@playwright/test'

// The tour, from Settings to its last stop, at a desktop
// width and a phone's; and the one-time offer, left with Esc.

const TITLES = [
  "Say what you're up for",
  'Your lineup',
  'Every underlined word is a menu',
  'More filters',
  'Covers or a table',
  'Every game has a sheet',
  'Series, in order',
  'Lists',
  'History',
  'Keys, and finding this again',
]
const PATHS = ['/', '/', '/library', '/library', '/library', '/library', '/collections', '/lists', '/history', '/']

const tourState = async (page: Page) => (await (await page.request.get('/api/v1/setup/state')).json()) as { tour: string; tour_offer: boolean }

/** The card sits inside the window, and so does the highlight's ring. On a
 *  desktop the card is beside the highlight, never over it; on a phone it
 *  sits at the bottom. */
async function cardInView(page: Page, what: string) {
  const card = page.locator('#tour-title').locator('..')
  const box = (await card.boundingBox())!
  const vp = page.viewportSize()!
  expect(box.x, what).toBeGreaterThanOrEqual(0)
  expect(box.y, what).toBeGreaterThanOrEqual(0)
  expect(box.x + box.width, what).toBeLessThanOrEqual(vp.width)
  expect(box.y + box.height, what).toBeLessThanOrEqual(vp.height)
  // The highlight slides into place, so ask until it's there.
  await expect
    .poll(async () => {
      const spot = (await page.locator('[data-tour-spot]').boundingBox())!
      const c = (await card.boundingBox())!
      const wrong: string[] = []
      if (spot.x < 2 || spot.y < 2 || spot.x + spot.width > vp.width - 2 || spot.y + spot.height > vp.height - 2) wrong.push('ring off screen')
      const apart =
        c.x >= spot.x + spot.width || spot.x >= c.x + c.width || c.y >= spot.y + spot.height || spot.y >= c.y + c.height
      if (vp.width >= 640 && !apart) wrong.push('card over the highlight')
      return wrong
    }, { message: what })
    .toEqual([])
}

for (const width of [1280, 390]) {
  test(`the tour from Settings to the end at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height: width > 600 ? 800 : 844 })
    const errors: string[] = []
    page.on('pageerror', (e) => errors.push(e.message))

    // At 1280, with the smoke test's lineup cleared: the lineup stop
    // shows an example. At 390 the new lineup stays, for the other words.
    // Wait for the last lineup to load before looking for Clear.
    if (width > 600) {
      const history = page.waitForResponse((r) => r.url().includes('/api/v1/picks/history?limit=1'))
      await page.goto('/')
      const last = (await (await history).json()) as unknown[]
      if (last.length) await page.getByRole('button', { name: 'Clear', exact: true }).click()
      await expect(page.locator('[data-tour="lineup"]')).toHaveAttribute('data-state', 'empty')
    }
    await page.goto('/settings')
    await page.getByRole('link', { name: 'Show me around' }).click()
    const tour = page.getByRole('dialog', { name: TITLES[0] })
    await expect(tour).toBeVisible()
    await expect(page).toHaveURL(/\/$/)
    await expect(page.locator('#tour-title')).toBeFocused()

    for (let i = 0; i < TITLES.length; i++) {
      const dialog = page.getByRole('dialog', { name: TITLES[i] })
      await expect(dialog, TITLES[i]).toBeVisible()
      await expect(dialog.getByText(`${i + 1} of ${TITLES.length}`)).toBeVisible()
      await expect(page).toHaveURL(new RegExp(`${PATHS[i] === '/' ? '/' : PATHS[i]}(\\?.*)?$`))
      await expect(page.locator('#tour-title')).toBeFocused()
      if (i === 1) {
        // With no lineup, the stop fills one from UpNext's own ranking as an
        // example, with no AI call and nothing to press.
        if (width > 600) await expect(dialog.getByText(/Three from your library as an example/)).toBeVisible()
        else await expect(dialog.getByText('Each pick says why it fits')).toBeVisible()
        await expect(page.locator('[data-tour="lineup"]')).toHaveAttribute('data-state', 'filled')
        await expect(dialog.getByRole('button')).toHaveCount(3)
      }
      if (i === 3) await expect(page.getByLabel('Genre')).toBeVisible()
      if (i === 5) {
        // The sheet opens on a game, under the tour's card.
        await expect(page.getByRole('dialog').filter({ has: page.getByRole('button', { name: 'Close' }) })).toBeVisible()
      }
      await expect(page.locator('[data-tour-spot]'), TITLES[i]).toBeVisible()
      await cardInView(page, TITLES[i])
      await dialog.getByRole('button', { name: i === TITLES.length - 1 ? 'Done' : 'Next' }).click()
    }
    await expect(page.getByRole('dialog')).toHaveCount(0)
    expect((await tourState(page)).tour).toBe('done')
    expect(errors).toEqual([])
  })
}

test('the one-time offer, and Esc to leave', async ({ page }) => {
  await page.request.put('/api/v1/setup', { data: { tour: 'open' } })
  await page.goto('/library')
  const offer = page.getByRole('region', { name: 'Tour' })
  await offer.getByRole('button', { name: 'Show me around' }).click()
  await expect(page.getByRole('dialog', { name: TITLES[0] })).toBeVisible()
  await expect(page).toHaveURL(/\/$/)
  await page.keyboard.press('ArrowRight')
  await expect(page.getByRole('dialog', { name: TITLES[1] })).toBeVisible()
  await page.keyboard.press('ArrowLeft')
  await expect(page.getByRole('dialog', { name: TITLES[0] })).toBeVisible()
  // Tab stays in the card.
  for (let i = 0; i < 4; i++) await page.keyboard.press('Tab')
  await expect(page.getByRole('dialog').locator(':focus')).toHaveCount(1)
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await expect(offer).toHaveCount(0)
  // The offer's button is gone, so focus goes to the top of the page.
  await expect(page.getByRole('link', { name: 'UpNext home' })).toBeFocused()
  expect(await tourState(page)).toMatchObject({ tour: 'skipped', tour_offer: false })
})

test('?tour=1 starts the tour from any page', async ({ page }) => {
  await page.goto('/library?view=table&tour=1')
  await expect(page.getByRole('dialog', { name: TITLES[0] })).toBeVisible()
  await expect(page).toHaveURL(/\/$/)
  await expect(page.locator('[data-tour-spot]')).toBeVisible()
  await page.keyboard.press('ArrowRight')
  await expect(page.getByRole('dialog', { name: TITLES[1] })).toBeVisible()
  await expect(page.locator('[data-tour-spot]')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await expect(page.getByRole('link', { name: 'UpNext home' })).toBeFocused()
})
