import { expect, test } from '@playwright/test'

// Allowing a device with a tap. The test server listens
// on this computer alone, so the page is told other devices are allowed, as
// Settings would say once they are; the device asking is Playwright's own
// request context, from the same computer, which the server accepts as it
// accepts any home-network address.

test('a phone asks, the computer allows it with a tap, and the phone is in', async ({ page, request }) => {
  await page.route('**/api/v1/settings', async (route) => {
    if (route.request().method() !== 'GET') return route.continue()
    const res = await route.fetch()
    const json = await res.json()
    json.listen = { ...json.listen, other_devices: true }
    await route.fulfill({ response: res, json })
  })
  await page.goto('/library')
  await expect(page.getByRole('link', { name: 'UpNext home' })).toBeVisible()

  const ask = await request.post('/api/v1/pair/request', { data: { name: 'Pixel 9', kind: 'phone' } })
  expect(ask.status()).toBe(201)
  const asked = (await ask.json()) as { id: string; secret: string; match_code: string }
  const collect = () => request.get(`/api/v1/pair/request/${asked.id}`, { headers: { 'UpNext-Pair-Secret': asked.secret } })
  expect(((await (await collect()).json()) as { status: string }).status).toBe('waiting')

  const dialog = page.getByRole('alertdialog', { name: 'A phone called “Pixel 9” wants to use UpNext' })
  await expect(dialog).toBeVisible()
  await expect(dialog).toContainText(asked.match_code)
  await dialog.getByRole('button', { name: 'Allow' }).click()
  await expect(dialog).toHaveCount(0)

  const answer = await collect()
  const got = (await answer.json()) as { status: string; id: number; name: string }
  expect([got.status, got.name]).toEqual(['allowed', 'Pixel 9'])
  expect(answer.headers()['set-cookie']).toMatch(/upnext_device_\d+=/)

  // Leave the server as it was for the other tests. The request now carries
  // the phone's cookie, so its write says where it came from, as a page does.
  const revoke = await request.delete(`/api/v1/tokens/${got.id}`, { headers: { Origin: new URL(page.url()).origin } })
  expect(revoke.status()).toBe(204)
})

test('the app window lists the computers on the network, and this one is a tap away', async ({ page }) => {
  await page.addInitScript(`
    window.upnextApp = true
    window.ipc = { postMessage: (m) => { window.sent = [...(window.sent ?? []), m] } }
  `)
  await page.goto('/connect')
  await expect(page.getByRole('heading', { name: 'Use the library on another computer' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'On your network' })).toBeVisible()
  await expect(page.getByPlaceholder('den.lan or 192.168.0.42:8787')).toBeVisible()
  await page.getByRole('button', { name: 'Use this computer’s library' }).last().click()
  // The page also tells the window whether it's in big screen; only the connect messages count here.
  expect(await page.evaluate<string[]>("window.sent.filter((m) => m.startsWith('connect:'))")).toEqual(['connect:here'])
})
