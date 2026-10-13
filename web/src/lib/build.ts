/** "UpNext 0.1.0, build abc1234": the server's version and the commit it was built from, when it says. */
export function buildText(health: { version: string; build?: string | null }): string {
  return `UpNext ${health.version}${health.build ? `, build ${health.build}` : ''}`
}
