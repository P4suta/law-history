import { defineConfig } from "@playwright/test";

const SERVER_ORIGIN = "http://127.0.0.1:37651";
const DESKTOP_WIDTH = 1280;
const DESKTOP_HEIGHT = 900;

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  use: {
    baseURL: SERVER_ORIGIN,
    browserName: "chromium",
    viewport: { width: DESKTOP_WIDTH, height: DESKTOP_HEIGHT },
    trace: "retain-on-failure",
  },
  webServer: {
    command: "../target/debug/xtask browser-server",
    url: `${SERVER_ORIGIN}/api/status`,
    reuseExistingServer: false,
  },
});
