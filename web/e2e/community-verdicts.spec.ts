import { expect, test } from '@playwright/test'

// The mock library comes with other players' verdicts
// (src/mock/verdicts.rs), so a game page shows them, the library filters by
// them and its table shows their headline, and Settings downloads the
// snapshot again from the fake service (e2e/fake-verdicts.mjs).
test("other players' backlog verdicts on a game, in the library and in Settings", async ({ page, request }) => {
  await page.goto('/library?view=table&status=everything&backlog=respects_time')
  await expect(page.getByRole('combobox', { name: 'Backlog players say' })).toHaveValue('respects_time')
  const row = page.getByRole('row').filter({ hasText: 'Stardew Valley' })
  await expect(row).toBeVisible()
  await page.setViewportSize({ width: 1440, height: 900 })
  await expect(row).toContainText(/Worth it, \d+ of 26/)

  await row.getByRole('button', { name: 'Stardew Valley' }).click()
  const sheet = page.getByRole('dialog', { name: 'Stardew Valley' })
  const section = sheet.getByRole('region', { name: 'Backlog verdicts' })
  await expect(section.getByText(/^Worth it on a backlog, say \d+ of 26 players$/)).toBeVisible()
  await expect(section.getByText('How far people got')).toBeVisible()
  await expect(section.getByText('From 26 UpNext players, updated 1 October 2026. Public under')).toBeVisible()
  await expect(section.getByRole('button', { name: 'Write a backlog verdict' })).toBeVisible()
  await page.keyboard.press('Escape')

  const before = (await (await request.get(`http://127.0.0.1:${process.env.FAKE_LLM_PORT ?? 8799}/verdicts/_seen`)).json()).snapshot_asked
  await page.goto('/settings#data')
  await expect(page.getByText(/· 28 of your games, updated 1 October 2026/)).toBeVisible()
  await page.getByRole('button', { name: 'Download now' }).click()
  await expect
    .poll(async () => (await (await request.get(`http://127.0.0.1:${process.env.FAKE_LLM_PORT ?? 8799}/verdicts/_seen`)).json()).snapshot_asked)
    .toBeGreaterThan(before)
})
