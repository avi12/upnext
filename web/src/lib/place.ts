/** Settings' sections as the page names them, with each one's anchor. */
export const SETTINGS_SECTIONS = {
  Setup: 'setup',
  Accounts: 'accounts',
  'Library data': 'data',
  Jobs: 'jobs',
  Backups: 'backups',
  Picks: 'picks',
  Profile: 'profile',
  'AI usage': 'ai-usage',
  'Devices and API': 'devices',
  'Linux and looks': 'linux',
  About: 'about',
} as const

export type SettingsSection = keyof typeof SETTINGS_SECTIONS

/** "Settings › Jobs": a place written as a path, not a list. */
export function placeText(section: SettingsSection): string {
  return `Settings › ${section}`
}
