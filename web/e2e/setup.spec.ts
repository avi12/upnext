import { expect, test, type Page } from '@playwright/test'

// The setup wizard: leaving the store steps (the second is Epic, EA
// and Ubisoft) starts the sync in the background, the First sync step shows it without holding Next, and going
// back and on again with the same accounts doesn't sync twice. The mock
// library has no Steam or Heroic folder here, so the sync reads nothing and
// its games stay; every game has its details, so none are fetched.

type JobRow = { task: string | null; source: string; status: string }

const wizardSyncs = async (page: Page) => {
  const jobs = (await (await page.request.get('/api/v1/jobs?limit=200')).json()) as { recent: JobRow[] }
  return jobs.recent.filter((j) => j.task === 'sync' && j.source === 'ui').length
}

const heading = (page: Page, name: string) => page.getByRole('heading', { level: 1, name, exact: true })

test('the wizard syncs in the background once the stores are behind', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(e.message))
  const before = await wizardSyncs(page)

  await page.goto('/setup?again=1')
  await page.getByRole('button', { name: 'Start' }).click()
  await expect(heading(page, 'Where UpNext runs')).toBeVisible()
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await expect(heading(page, 'Steam')).toBeVisible()
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await expect(heading(page, 'GOG, Epic and Amazon')).toBeVisible()
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await expect(heading(page, 'Epic, EA and Ubisoft')).toBeVisible()
  await expect(page.getByText('Next starts the first sync in the background')).toBeVisible()
  expect(await wizardSyncs(page)).toBe(before)

  // Leaving the stores starts the sync; the person goes on meanwhile.
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await expect(heading(page, 'Extras')).toBeVisible()
  await expect.poll(() => wizardSyncs(page)).toBe(before + 1)

  // Back to the stores and on again, with nothing changed: no second sync.
  await page.getByRole('button', { name: 'Back', exact: true }).click()
  await expect(heading(page, 'Epic, EA and Ubisoft')).toBeVisible()
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await expect(heading(page, 'Extras')).toBeVisible()

  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await expect(heading(page, 'AI')).toBeVisible()
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await expect(heading(page, 'What the AI does')).toBeVisible()
  await page.getByRole('button', { name: 'Next', exact: true }).click()

  // The First sync step has what came in, and Next isn't held.
  await expect(heading(page, 'First sync')).toBeVisible()
  await expect(page.getByText(/^103 games/)).toBeVisible()
  // The EA app's and Ubisoft Connect's installs read before stay, uninstalled.
  await expect(page.getByText(/^Last sync: 2 games, 0 new/)).toBeVisible()
  await expect(page.getByRole('button', { name: 'Next', exact: true })).toBeEnabled()
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await expect(heading(page, 'Details')).toBeVisible()
  await expect(page.getByText('Every game has its details.')).toBeVisible()
  expect(await wizardSyncs(page)).toBe(before + 1)

  // "Sync again" goes through the same background work.
  await page.getByRole('button', { name: 'Back', exact: true }).click()
  await page.getByRole('button', { name: 'Sync again' }).click()
  await expect.poll(() => wizardSyncs(page)).toBe(before + 2)
  await expect(page.getByRole('button', { name: 'Sync again' })).toBeVisible()

  await page.getByRole('button', { name: /Leave setup|Skip setup/ }).click()
  await expect(page).toHaveURL(/\/$/)
  expect(errors).toEqual([])
})
