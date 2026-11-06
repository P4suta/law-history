import { defineConfig } from "@playwright/test";

const PAGES_BASE_URL = "http://127.0.0.1:37652/law-history/";
const DEFAULT_SERVER_COMMAND = "../target/debug/xtask pages-server";
const DESKTOP_WIDTH = 1280;
const DESKTOP_HEIGHT = 900;
const SERVER_STARTUP_TIMEOUT_MS = 10_000;

export default defineConfig({
  testDir: "./pages-tests",
  workers: 1,
  retries: 0,
  use: {
    baseURL: PAGES_BASE_URL,
    browserName: "chromium",
    viewport: { width: DESKTOP_WIDTH, height: DESKTOP_HEIGHT },
    trace: "retain-on-failure",
  },
  webServer: {
    command: process.env.LAW_HISTORY_XTASK
      ? '"$LAW_HISTORY_XTASK" pages-server'
      : DEFAULT_SERVER_COMMAND,
    url: PAGES_BASE_URL,
    timeout: SERVER_STARTUP_TIMEOUT_MS,
    reuseExistingServer: false,
  },
});
