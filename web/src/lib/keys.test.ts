import { afterEach, describe, expect, it } from 'vitest'
import { shortcutBlocked, typingInField } from './keys'

const key = (target: EventTarget, init: KeyboardEventInit = {}) => {
  const e = new KeyboardEvent('keydown', { key: '1', ...init })
  Object.defineProperty(e, 'target', { value: target })
  return e
}

describe('keys', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('knows when someone is typing', () => {
    expect(typingInField(document.createElement('input'))).toBe(true)
    expect(typingInField(document.createElement('textarea'))).toBe(true)
    expect(typingInField(document.createElement('select'))).toBe(true)
    // jsdom has no isContentEditable, so this is undefined there and false in a browser.
    expect(typingInField(document.createElement('button'))).toBeFalsy()
    expect(typingInField(null)).toBe(false)
  })

  it('blocks page shortcuts while typing, with a modifier, or under a menu or dialog', () => {
    const body = document.body
    expect(shortcutBlocked(key(body))).toBe(false)
    expect(shortcutBlocked(key(document.createElement('input')))).toBe(true)
    expect(shortcutBlocked(key(body, { ctrlKey: true }))).toBe(true)
    expect(shortcutBlocked(key(body, { metaKey: true }))).toBe(true)
    expect(shortcutBlocked(key(body, { altKey: true }))).toBe(true)
    for (const role of ['dialog', 'menu', 'listbox']) {
      document.body.innerHTML = `<div role="${role}"></div>`
      expect(shortcutBlocked(key(body)), role).toBe(true)
    }
  })
})
