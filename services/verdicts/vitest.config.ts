import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["test/**/*.test.ts"],
    environment: "node",
    // Each file starts its own workerd; give it time on a slow machine.
    testTimeout: 30_000,
    hookTimeout: 60_000,
  },
});
