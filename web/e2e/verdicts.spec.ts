import { expect, test } from '@playwright/test'

// After Beat it, write a backlog verdict, accept the consent, send
// it to the fake service (e2e/fake-verdicts.mjs), see it as yours, take it down.
// It runs once per server: the consent it accepts, the mark and the verdict it
// takes down stay in the database (the API has no way to take a consent back),
// so a second run on the same server (`--repeat-each`) starts somewhere else.
// `npm run e2e` starts fresh servers each time.
test('write a backlog verdict after Beat it, send it and take it down', async ({ page, request }) => {
  await page.goto('/')
  await page.getByLabel('What are you in the mood for?').fill('Something to finish')
  await page.getByRole('button', { name: 'Curate', exact: true }).click()
  const opens = page.getByRole('region', { name: 'Lineup' }).getByRole('button', { name: /^Open / })
  await expect(opens).toHaveCount(3)

  let sheet = null
  for (const open of await opens.all()) {
    const name = (await open.getAttribute('aria-label'))!.replace(/^Open /, '')
    await open.click()
    const s = page.getByRole('dialog', { name })
    await expect(s).toBeVisible()
    await s.getByRole('button', { name: /Mark as/ }).click()
    // Every kind's menu has Not for me: once it shows, the menu is complete
    // and the count of Beat it is what the game offers, not a race.
    const menu = s.getByRole('menu', { name: 'Mark this game as' })
    await expect(menu.getByRole('menuitemradio', { name: /Not for me/ })).toBeVisible()
    const beat = menu.getByRole('menuitemradio', { name: /Beat it/ })
    if (await beat.count()) {
      await beat.click()
      sheet = s
      break
    }
    await page.keyboard.press('Escape')
    await page.keyboard.press('Escape')
    await expect(s).toBeHidden()
  }
  expect(sheet, 'one of the picks has an ending').not.toBeNull()

  await expect(sheet!.getByText('Tell other backlog players how it went?')).toBeVisible()
  await sheet!.getByRole('button', { name: 'Write a verdict' }).click()
  const form = sheet!.getByRole('form', { name: 'Your backlog verdict' })
  await expect(form.getByRole('button', { name: 'Finished it' })).toHaveAttribute('aria-pressed', 'true')
  await form.getByRole('group', { name: /Worth it/ }).getByRole('button', { name: 'Yes' }).click()
  await form.getByRole('textbox', { name: /Anything else/ }).fill('Short chapters, easy to pick back up.')
  await form.getByRole('button', { name: 'Send' }).click()

  await expect(sheet!.getByText('Your verdict will be public.')).toBeVisible()
  await sheet!.getByRole('button', { name: 'Publish my verdicts under CC0' }).click()
  await expect(sheet!.getByText(/^Your verdict:/)).toBeVisible()
  await expect(sheet!.getByText(/^Your verdict:/)).toContainText('Sent')

  const seen = await (await request.get(`http://127.0.0.1:${process.env.FAKE_LLM_PORT ?? 8799}/verdicts/_seen`)).json()
  expect(seen.verdicts).toHaveLength(1)
  expect(seen.verdicts[0]).toMatchObject({ outcome: 'finished', worth: 'yes', text: 'Short chapters, easy to pick back up.', licence: 'CC0-1.0' })

  await sheet!.getByRole('button', { name: 'Take down' }).click()
  await expect(sheet!.getByText(/left out of the next daily update/)).toBeVisible()
  const after = await (await request.get(`http://127.0.0.1:${process.env.FAKE_LLM_PORT ?? 8799}/verdicts/_seen`)).json()
  expect(after.verdicts).toHaveLength(0)
})
