import { expect, test } from '@playwright/test'

// The mock library has two players, the first and Sam,
// and the first hid Hotline Miami from everyone else. A browser that has
// never chosen asks who's playing; the choice sticks.
test.use({ storageState: { cookies: [], origins: [] } })

test("a new browser asks who's playing, and Sam's library leaves out what the first player hid", async ({ page }) => {
  await page.goto('/library')
  const picker = page.getByRole('dialog', { name: 'Who’s playing?' })
  await expect(picker).toBeVisible()
  await picker.getByRole('button', { name: 'Sam', exact: true }).click()
  await expect(picker).toBeHidden()
  await expect(page.getByRole('button', { name: 'Playing as Sam' })).toBeVisible()

  // The player cookie came with the choice, so the API answers for Sam.
  const found = await page.request.get('/api/v1/games?q=Hotline')
  expect(((await found.json()).items as { name: string }[]).map((g) => g.name)).not.toContain('Hotline Miami')
  const current = await page.request.get('/api/v1/players/current')
  expect((await current.json()).player.name).toBe('Sam')

  // Remembered: no picker after a reload, and Lists has Hidden by me.
  await page.goto('/lists')
  await expect(page.getByRole('button', { name: 'Playing as Sam' })).toBeVisible()
  await expect(picker).toBeHidden()
  await expect(page.getByRole('button', { name: /^Hidden by me/ })).toBeVisible()
})

// A household from before the wizard asked for a name, whose
// first player is still "Player 1", is told once where to change it.
test('the made-up "Player 1" is pointed to Settings › Players once', async ({ page }) => {
  await page.goto('/library')
  const picker = page.getByRole('dialog', { name: 'Who’s playing?' })
  await picker.getByRole('button', { name: 'Player 1', exact: true }).click()
  await expect(picker).toBeHidden()
  const nudge = page.getByText('You’re “Player 1” here')
  await expect(nudge).toBeVisible()
  await page.getByRole('button', { name: 'Change it' }).click()
  await expect(page).toHaveURL(/\/settings#players$/)
  await expect(page.getByText('The name UpNext gave you, which “Who’s playing?” shows. Change gives you your own.')).toBeVisible()
  await page.goto('/library')
  await expect(page.getByRole('button', { name: 'Playing as Player 1' })).toBeVisible()
  await expect(nudge).toHaveCount(0)
})
