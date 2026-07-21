import { defineConfig } from "@playwright/test";

const TEST_PORT = 1421;
const TEST_BASE_URL = `http://127.0.0.1:${TEST_PORT}`;

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  use: {
    baseURL: TEST_BASE_URL,
    trace: "on-first-retry"
  },
  webServer: {
    command: `npm run dev -- --port ${TEST_PORT}`,
    url: TEST_BASE_URL,
    reuseExistingServer: !process.env.CI
  }
});
