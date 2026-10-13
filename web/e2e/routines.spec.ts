import { expect, test } from '@playwright/test'

// Routines: make one on the Profile, choose it on the Pick page, and
// see the lineup and History say what it was picked for.
test('make a routine and curate for it', async ({ page, request }) => {
  await page.goto('/profile#routines')
  const section = page.locator('#routines')
  // The mock library comes with three routines.
  await expect(section.getByText('Saturday co-op')).toBeVisible()
  await section.getByRole('button', { name: 'Add a routine' }).click()
  const form = section.getByRole('form', { name: 'New routine' })
  await form.getByLabel('Name').fill('Late with a pad')
  await form.getByRole('button', { name: 'Late nights' }).click()
  await form.getByRole('button', { name: 'On my own' }).click()
  await form.getByRole('button', { name: 'A controller' }).click()
  await form.getByRole('button', { name: 'an hour', exact: true }).click()
  await form.getByRole('button', { name: 'Save' }).click()
  await expect(section.getByText('Late nights · on my own · on a controller · 1h')).toBeVisible()

  const saved = await (await request.get('/api/v1/routines')).json()
  expect(saved.routines.find((r: { name: string }) => r.name === 'Late with a pad')).toMatchObject({
    times: ['late_nights'],
    who: 'solo',
    input: 'controller',
    minutes: 60,
  })

  await page.goto('/')
  const chips = page.getByRole('group', { name: 'Routine' })
  const coop = chips.getByRole('button', { name: /Saturday co-op/ })
  // Whichever routine fits the time the test runs is chosen at first.
  if ((await coop.getAttribute('aria-pressed')) !== 'true') await coop.click()
  await expect(coop).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByText(/Picks fit “Saturday co-op”/)).toBeVisible()
  await page.getByRole('button', { name: 'Curate', exact: true }).click()

  const lineup = page.getByRole('region', { name: 'Lineup' })
  await expect(lineup.getByRole('button', { name: /^Open / })).toHaveCount(3)
  await expect(lineup.getByText(/For your routine “Saturday co-op”: Saturday afternoons and evenings · with friends/)).toBeVisible()

  const last = (await (await request.get('/api/v1/picks/history?limit=1')).json())[0]
  expect(last.routine.name).toBe('Saturday co-op')

  await page.goto('/history')
  await expect(page.getByText(/· for “Saturday co-op”/).first()).toBeVisible()
})
