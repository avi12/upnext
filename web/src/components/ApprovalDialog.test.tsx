import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ReactNode } from 'react'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../api'
import type { Settings, WaitingDevices } from '../types'
import { ApprovalDialog, ARM_MS } from './ApprovalDialog'

const allowButton = () => screen.getByRole('button', { name: 'Allow' }) as HTMLButtonElement
/** Allow, once it can be pressed. */
async function allow() {
  await waitFor(() => expect(allowButton().disabled).toBe(false), { timeout: ARM_MS + 1000 })
  fireEvent.click(allowButton())
}

const phone = { id: 'r1', name: 'Pixel 9', kind: 'phone', match_code: '4821', from: '192.168.0.42', via: null, expires_at: 0 } as const

function show(listen: { this_computer: boolean; other_devices: boolean }, waiting: WaitingDevices, behind?: ReactNode) {
  vi.spyOn(api, 'settings').mockResolvedValue({ listen } as unknown as Settings)
  const asked = vi.spyOn(api, 'pairingRequests').mockResolvedValue(waiting)
  const answer = vi.spyOn(api, 'answerPairingRequest').mockResolvedValue(undefined as never)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      {behind}
      <ApprovalDialog />
    </QueryClientProvider>,
  )
  return { asked, answer }
}

describe('the Allow dialog', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    delete window.upnextRemote
  })

  it('shows a device asking, with its number, and the focus starts on Deny', async () => {
    const { answer } = show({ this_computer: true, other_devices: true }, { requests: [phone], players: [] })
    const dialog = await screen.findByRole('alertdialog', { name: 'A phone called “Pixel 9” wants to use UpNext' })
    expect(dialog.textContent).toContain('4821')
    expect(screen.queryByText('Whose device is it?')).toBeNull()
    // The focus engine starts on the marked button: Deny, so a stray press allows nothing.
    expect(screen.getByRole('button', { name: 'Deny' }).hasAttribute('data-focus-first')).toBe(true)
    await allow()
    await waitFor(() => expect(answer).toHaveBeenCalledWith('r1', true, null, null))
  })

  it('says where the device asked from, and the proxy it came through (B-AUTH-2)', async () => {
    show({ this_computer: true, other_devices: true }, { requests: [{ ...phone, from: '100.64.0.7', via: '127.0.0.1' }], players: [] })
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog.textContent).toContain('From 100.64.0.7 through the proxy at 127.0.0.1 on your network.')
  })

  it('says the address of a device that asked directly', async () => {
    show({ this_computer: true, other_devices: true }, { requests: [phone], players: [] })
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog.textContent).toContain('From 192.168.0.42 on your network.')
  })

  it('with two players, asks whose device it is', async () => {
    const players = [
      { id: 1, name: 'Sam' },
      { id: 2, name: 'Alex' },
    ]
    const { answer } = show({ this_computer: true, other_devices: true }, { requests: [phone], players })
    fireEvent.click(await screen.findByRole('button', { name: 'Alex' }))
    expect(screen.getByRole('button', { name: 'Alex' }).getAttribute('aria-pressed')).toBe('true')
    await allow()
    await waitFor(() => expect(answer).toHaveBeenCalledWith('r1', true, 2, null))
    fireEvent.click(screen.getByRole('button', { name: 'Everyone’s' }))
    fireEvent.click(screen.getByRole('button', { name: 'Deny' }))
    await waitFor(() => expect(answer).toHaveBeenLastCalledWith('r1', false, null, null))
  })

  it("allowing a device as a player with a PIN takes their PIN", async () => {
    vi.spyOn(api, 'players').mockResolvedValue({
      players: [
        { id: 1, name: 'Sam', colour: '#4f8cff', has_pin: false, created_at: 0, default: true },
        { id: 2, name: 'Alex', colour: '#f2994a', has_pin: true, created_at: 0, default: false },
      ],
      current: 1,
      bound: false,
    })
    const { answer } = show({ this_computer: true, other_devices: true }, { requests: [phone], players: [{ id: 1, name: 'Sam' }, { id: 2, name: 'Alex' }] })
    fireEvent.click(await screen.findByRole('button', { name: 'Alex' }))
    const pin = await screen.findByLabelText('Alex’s PIN')
    await new Promise((r) => setTimeout(r, ARM_MS + 50))
    expect(allowButton().disabled).toBe(true)
    fireEvent.change(pin, { target: { value: '2468' } })
    await allow()
    await waitFor(() => expect(answer).toHaveBeenCalledWith('r1', true, 2, '2468'))
  })

  it('keeps Allow off for a moment after another request takes the place of the one shown', async () => {
    const tablet = { ...phone, id: 'r2', name: 'Tab S9', kind: 'tablet', match_code: '1234' } as const
    const { asked, answer } = show({ this_computer: true, other_devices: true }, { requests: [phone], players: [] })
    await screen.findByRole('alertdialog', { name: /Pixel 9/ })
    await waitFor(() => expect(allowButton().disabled).toBe(false), { timeout: ARM_MS + 1000 })
    // The phone gives up and the tablet asks, between two looks.
    asked.mockResolvedValue({ requests: [tablet], players: [] } as WaitingDevices)
    await screen.findByRole('alertdialog', { name: /Tab S9/ }, { timeout: 4000 })
    fireEvent.click(allowButton())
    expect(allowButton().disabled).toBe(true)
    expect(answer).not.toHaveBeenCalled()
    await allow()
    await waitFor(() => expect(answer).toHaveBeenCalledWith('r2', true, null, null))
  })

  it('keeps Tab inside the dialog, both ways (B-WEB-6)', async () => {
    const players = [
      { id: 1, name: 'Sam' },
      { id: 2, name: 'Alex' },
    ]
    show({ this_computer: true, other_devices: true }, { requests: [phone], players }, <button type="button">On the page behind</button>)
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog.getAttribute('aria-modal')).toBe('true')
    await waitFor(() => expect(allowButton().disabled).toBe(false), { timeout: ARM_MS + 1000 })
    screen.getByRole('button', { name: 'Deny' }).focus()
    const user = userEvent.setup()
    const reached: string[] = []
    for (let i = 0; i < 6; i++) {
      await user.tab()
      reached.push(document.activeElement?.textContent ?? '')
    }
    for (let i = 0; i < 6; i++) {
      await user.tab({ shift: true })
      reached.push(document.activeElement?.textContent ?? '')
    }
    expect(reached).not.toContain('On the page behind')
    expect(reached).toContain('Sam')
    expect(reached).toContain('Allow')
  })

  it('gives the focus back where it was when it goes', async () => {
    const { asked } = show({ this_computer: true, other_devices: true }, { requests: [phone], players: [] }, <button type="button">On the page behind</button>)
    const behind = screen.getByRole('button', { name: 'On the page behind' })
    behind.focus()
    await screen.findByRole('alertdialog')
    // The focus engine puts it on Deny in a browser; jsdom lays nothing out, so it's put there here.
    screen.getByRole('button', { name: 'Deny' }).focus()
    asked.mockResolvedValue({ requests: [], players: [] })
    fireEvent.click(screen.getByRole('button', { name: 'Deny' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull())
    expect(document.activeElement).toBe(behind)
  })

  it('says what to check and spells the number digit by digit', async () => {
    show({ this_computer: true, other_devices: true }, { requests: [phone], players: [] })
    // Read as the dialog opens, after its title: the instruction, then the
    // digits one by one rather than "four thousand eight hundred …".
    const dialog = await screen.findByRole('alertdialog', { description: 'Allow it only if it shows this number: 4 8 2 1' })
    expect(dialog.querySelector('[aria-hidden="true"]')?.textContent).toBe('4821')
  })

  it('makes the page behind inert while it shows, and frees it after', async () => {
    const { asked } = show(
      { this_computer: true, other_devices: true },
      { requests: [phone], players: [] },
      <>
        <button type="button">On the page behind</button>
        <div inert>Inert already</div>
      </>,
    )
    await screen.findByRole('alertdialog')
    const behind = screen.getByText('On the page behind')
    const already = screen.getByText('Inert already')
    // Out of reach of a screen reader's arrow keys, which aria-modal alone didn't stop in Orca.
    expect(behind.hasAttribute('inert')).toBe(true)
    expect(screen.getByRole('alertdialog').closest('[inert]')).toBeNull()
    asked.mockResolvedValue({ requests: [], players: [] })
    fireEvent.click(screen.getByRole('button', { name: 'Deny' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull())
    expect(behind.hasAttribute('inert')).toBe(false)
    expect(already.hasAttribute('inert')).toBe(true)
  })

  it('Escape denies (B-WEB-6)', async () => {
    const { answer } = show({ this_computer: true, other_devices: true }, { requests: [phone], players: [] })
    await screen.findByRole('alertdialog')
    fireEvent.keyDown(window, { key: 'Escape' })
    await waitFor(() => expect(answer).toHaveBeenCalledWith('r1', false, null, null))
  })

  it('asks nothing elsewhere: another device, or a window on another computer', async () => {
    const { asked } = show({ this_computer: false, other_devices: true }, { requests: [phone], players: [] })
    await waitFor(() => expect(api.settings).toHaveBeenCalled())
    expect(asked).not.toHaveBeenCalled()
    expect(screen.queryByRole('alertdialog')).toBeNull()
  })

  it('asks nothing in a window showing another computer', async () => {
    window.upnextRemote = { name: 'den' }
    const { asked } = show({ this_computer: true, other_devices: true }, { requests: [phone], players: [] })
    await waitFor(() => expect(api.settings).toHaveBeenCalled())
    expect(asked).not.toHaveBeenCalled()
  })
})
