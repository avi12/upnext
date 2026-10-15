import { expect, test, type Locator, type Page } from '@playwright/test'
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
const largeOut = fileURLToPath(new URL('../test-results/pick-3200.png', import.meta.url))

// Every shot asks for reduced motion, which turns the cards' fade-in off: a
// shot taken as the covers load would otherwise catch the cards half drawn.

/** How many of the lineup's covers loaded: a cover that fails becomes a
 *  name card and drops its image, so three mean the three picks show art. */
function coversLoaded(lineup: Locator) {
  return () =>
    lineup
      .locator('img')
      .evaluateAll((imgs) => imgs.filter((i) => (i as HTMLImageElement).complete && (i as HTMLImageElement).naturalWidth > 0).length)
}

test.describe('big screen', () => {
  test.use({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1, reducedMotion: 'reduce' })

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

/** The Pick page after Curate on the mock library, as the README and the site show it. */
async function pickShot(page: Page, path: string) {
  await page.goto('/')
  // The first run's tour offer would push the picks' buttons out of the shot.
  // The server remembers the answer, so only a run's first shot sees it.
  const tour = page.getByRole('region', { name: 'Tour' })
  await page.waitForLoadState('networkidle')
  const noThanks = tour.getByRole('button', { name: 'No thanks' })
  if (await noThanks.isVisible()) await noThanks.click()
  await expect(tour).toHaveCount(0)
  await page.getByLabel('What are you in the mood for?').fill('About an hour, something calm with a bit of story')
  await page.getByRole('button', { name: 'Curate', exact: true }).click()
  const lineup = page.getByRole('region', { name: 'Lineup' })
  await expect(lineup.getByRole('button', { name: /^Open / })).toHaveCount(3)

  await expect.poll(coversLoaded(lineup), { message: "the picks' covers from Steam's image servers", timeout: 30_000 }).toBeGreaterThanOrEqual(3)
  await page.mouse.move(0, 0)
  await page.screenshot({ path })
}

test.describe('README', () => {
  test.use({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1.25, reducedMotion: 'reduce' })

  test('the README screenshot', async ({ page }) => {
    test.skip(!process.env.UPNEXT_README_SHOT, 'set UPNEXT_README_SHOT=1 to take docs/images/pick.png again')
    test.setTimeout(90_000)
    await pickShot(page, out)
  })
})

// The same page at twice the README's scale, 3200 × 2250, for the site's
// hero on wide and high-density screens. It goes under test-results, which
// git ignores; site/README.md says how the site's WebP copies are made from it.
test.describe('site', () => {
  test.use({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 2.5, reducedMotion: 'reduce' })

  test('the site’s large hero screenshot', async ({ page }) => {
    test.skip(!process.env.UPNEXT_README_SHOT, 'set UPNEXT_README_SHOT=1 to take web/test-results/pick-3200.png again')
    test.setTimeout(90_000)
    await pickShot(page, largeOut)
  })
})
