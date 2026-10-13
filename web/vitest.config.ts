import { defineConfig, mergeConfig } from 'vitest/config'
import viteConfig from './vite.config.ts'

export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      environment: 'jsdom',
      // Each test file in its own VM context on worker threads that keep
      // their jsdom: setting jsdom up for every file was over half the run
      // (code health R-WEB-8). Files stay isolated from one another.
      pool: 'vmThreads',
      include: ['src/**/*.test.{ts,tsx}'],
      setupFiles: ['src/test/setup.ts'],
    },
  }),
)
