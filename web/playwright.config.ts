import { defineConfig, devices } from '@playwright/test'

// The smoke test (e2e/): `upnext serve` on the mock library with a fake
// model, driven in Chromium. It needs a built UI (`npx vite build`) and a
// built binary (`cargo build`, or UPNEXT_BIN). `npm run e2e` runs it.
const port = Number(process.env.UPNEXT_PORT ?? 8798)
const fakePort = Number(process.env.FAKE_LLM_PORT ?? 8799)
// A second UpNext server, beside the fake model and the two-player one,
// with the mock library's first player alone: one-player.spec.ts
// runs against it.
const onePlayerPort = Number(process.env.UPNEXT_ONE_PLAYER_PORT ?? 8797)
const env = { ...process.env, UPNEXT_PORT: String(port), FAKE_LLM_PORT: String(fakePort) } as Record<string, string>
const onePlayerEnv = { ...env, UPNEXT_PORT: String(onePlayerPort), UPNEXT_E2E_ONE_PLAYER: '1' }

export default defineConfig({
  testDir: 'e2e',
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    // The mock library has two players: the browser has chosen the
    // first and doesn't ask each time (the app window would by default), so
    // "Who's playing?" stays away unless a test clears it. It has also been
    // told once that the first player's "Player 1" is a made-up name.
    storageState: {
      cookies: [],
      origins: [
        {
          origin: `http://127.0.0.1:${port}`,
          localStorage: [
            { name: 'upnext.player', value: '1' },
            { name: 'upnext.player.ask', value: '0' },
            { name: 'upnext.player.nudged', value: '1' },
          ],
        },
      ],
    },
    trace: 'retain-on-failure',
    ...devices['Desktop Chrome'],
    // A Chromium already on the machine, when Playwright's own isn't installed.
    launchOptions: process.env.PLAYWRIGHT_CHROMIUM ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM } : {},
  },
  webServer: [
    { command: 'node e2e/fake-llm.mjs', url: `http://127.0.0.1:${fakePort}/health`, env, reuseExistingServer: false },
    { command: 'e2e/serve.sh', url: `http://127.0.0.1:${port}/api/health`, env, reuseExistingServer: false, timeout: 60_000 },
    {
      command: 'e2e/serve.sh',
      url: `http://127.0.0.1:${onePlayerPort}/api/health`,
      env: onePlayerEnv,
      reuseExistingServer: false,
      timeout: 60_000,
    },
  ],
})
