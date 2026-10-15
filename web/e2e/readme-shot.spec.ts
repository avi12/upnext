import { expect, test, type Locator } from '@playwright/test'
import { fileURLToPath } from 'node:url'

// The README's screenshot, docs/images/pick.png: the Pick page after
// Curate on the mock library, at 1280 px and 1.25x, so 1600 × 1125. It runs
// only with UPNEXT_README_SHOT=1, on a machine that reaches Steam's image
// servers: the mock library's games are real ones, so their covers load from
// there, and CI and development containers often can't reach them. It fails rather than save
// a shot of name cards. Nothing in it comes from anyone's own library.
const out = fileURLToPath(new URL('../../docs/images/pick.png', import.meta.url))
// Big screen's, for the Flatpak's store listing, at a
// Steam Deck's 1280 × 800, taken the same way.
const bigOut = fileURLToPath(new URL('../../docs/images/big-screen.png', import.meta.url))

/** How many of the lineup's covers loaded: a cover that fails becomes a
 *  name card and drops its image, so three mean the three picks show art. */
function coversLoaded(lineup: Locator) {
  return () =>
    lineup
      .locator('img')
      .evaluateAll((imgs) => imgs.filter((i) => (i as HTMLImageElement).complete && (i as HTMLImageElement).naturalWidth > 0).length)
}

test.describe('big screen', () => {
  test.use({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 })

  test('the store listing’s big screen screenshot', async ({ page }) => {
    test.skip(!process.env.UPNEXT_README_SHOT, 'set UPNEXT_README_SHOT=1 to take docs/images/big-screen.png again')
    test.setTimeout(90_000)
    await page.goto('/?big=1')
    await page.keyboard.press('/')
    const request = page.getByRole('region', { name: 'Request' })
    await request.getByRole('button', { name: 'An hour', exact: true }).click()
    await page.getByRole('button', { name: 'Curate', exact: true }).click()
    const lineup = page.getByRole('region', { name: 'Lineup' })
    await expect(lineup.getByRole('article')).toHaveCount(3)
    await expect.poll(coversLoaded(lineup), { message: "the picks' covers from Steam's image servers", timeout: 30_000 }).toBeGreaterThanOrEqual(3)
    await page.mouse.move(0, 0)
    await page.screenshot({ path: bigOut })
  })
})

test.use({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1.25 })

test('the README screenshot', async ({ page }) => {
  test.skip(!process.env.UPNEXT_README_SHOT, 'set UPNEXT_README_SHOT=1 to take docs/images/pick.png again')
  test.setTimeout(90_000)
  await page.goto('/')
  // The first run's tour offer would push the picks' buttons out of the shot.
  const tour = page.getByRole('region', { name: 'Tour' })
  await tour.getByRole('button', { name: 'No thanks' }).click()
  await expect(tour).toHaveCount(0)
  await page.getByLabel('What are you in the mood for?').fill('About an hour, something calm with a bit of story')
  await page.getByRole('button', { name: 'Curate', exact: true }).click()
  const lineup = page.getByRole('region', { name: 'Lineup' })
  await expect(lineup.getByRole('button', { name: /^Open / })).toHaveCount(3)

  await expect.poll(coversLoaded(lineup), { message: "the picks' covers from Steam's image servers", timeout: 30_000 }).toBeGreaterThanOrEqual(3)
  await page.mouse.move(0, 0)
  await page.screenshot({ path: out })
})
