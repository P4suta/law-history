import { readFile } from "node:fs/promises";
import { expect, type Page, test } from "@playwright/test";
import { z } from "zod";

const EGOV_ORIGIN = "https://laws.e-gov.go.jp";
const EGOV_ROUTE = `${EGOV_ORIGIN}/api/2/**`;

async function sample(page: Page): Promise<void> {
  await page.goto("./");
  await page.getByRole("button", { name: "Load synthetic sample", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Demonstration Act", exact: true, level: 2 }),
  ).toBeVisible();
}

test("static WebAssembly runs history, comparison, references, and recovery without an API", async ({
  page,
}, testInfo) => {
  const api: string[] = [];
  page.on("request", (request): void => {
    if (new URL(request.url()).pathname.startsWith("/api/")) api.push(request.url());
  });
  await sample(page);
  const nativeCorpus = process.env.LAW_HISTORY_EXPECTED_CORPUS;
  if (!nativeCorpus)
    throw new Error(
      "Run the browser specification through cargo test --package xtask --test browser_workspace",
    );
  const exported = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export corpus", exact: true }).click();
  const corpus = await exported;
  const corpusFile = await corpus.path();
  if (!corpusFile) throw new Error("The browser corpus download is missing");
  expect(await readFile(corpusFile, "utf8")).toBe(await readFile(nativeCorpus, "utf8"));
  await page.getByRole("button", { name: "Compare", exact: true }).click();
  await expect(page.getByText("申請は書面で行う。", { exact: true })).toBeVisible();
  await expect(page.getByText("申請は電磁的記録で行う。", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Review through references" })).toBeVisible();
  expect(new URL(page.url()).searchParams.get("from")).toHaveLength(64);
  await page.reload();
  await expect(page.getByText("申請は書面で行う。", { exact: true })).toBeVisible();
  await page.getByLabel("Known at (RFC 3339)").fill("2026-01-01T12:00:00Z");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await page.getByRole("button", { name: "Demonstration Act", exact: true }).click();
  await page.getByText("Selected law text", { exact: true }).click();
  await expect(page.getByText("第一条申請は書面で行う。", { exact: true })).toBeVisible();
  expect(api).toEqual([]);
  expect((await page.request.get("/api/status")).status()).toBe(404);
  await page.screenshot({ path: testInfo.outputPath("pages-history.png"), fullPage: true });
});

test("explicit sharing retains the recipe and corpus imports retain exact pinned versions", async ({
  page,
  browser,
}, testInfo) => {
  await sample(page);
  await page.getByLabel("Enforcement date", { exact: true }).fill("1969-12-31");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await page.getByRole("button", { name: "Demonstration Act", exact: true }).click();
  await page.getByLabel("Original text", { exact: true }).fill("書面");
  await page.getByLabel("Replacement text", { exact: true }).fill("電磁的記録");
  await page.getByRole("button", { name: "Create scenario from selected revision" }).click();
  await page.getByRole("button", { name: "Simulate", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Simulation result", exact: true })).toBeVisible();
  expect(new URL(page.url()).searchParams.has("scenario")).toBe(false);
  await page.getByRole("button", { name: "Save scenario for sharing" }).click();
  await expect(page.getByText(/Scenario saved\./u)).toBeVisible();
  const shared = page.url();
  const recipe = await page.getByLabel("Scenario JSON").inputValue();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export corpus", exact: true }).click();
  const file = await download;
  const corpus = testInfo.outputPath("corpus.json");
  await file.saveAs(corpus);
  const context = await browser.newContext();
  const receiver = await context.newPage();
  await receiver.goto(shared);
  await expect(receiver.getByLabel("Scenario JSON")).toHaveValue(recipe);
  await receiver.getByRole("link", { name: "Skip to search", exact: true }).focus();
  await receiver.keyboard.press("Enter");
  expect(new URL(receiver.url()).hash).toBe(new URL(shared).hash);
  await receiver.getByRole("button", { name: "Simulate", exact: true }).click();
  await expect(receiver.getByRole("alert")).toBeVisible();
  await receiver.getByLabel("Import corpus or fixture").setInputFiles(corpus);
  await expect(receiver.getByText(/Corpus imported\./u)).toBeVisible();
  await receiver.getByRole("button", { name: "Simulate", exact: true }).click();
  await expect(
    receiver.getByRole("heading", { name: "Simulation result", exact: true }),
  ).toBeVisible();
  const restored: unknown = JSON.parse(await receiver.getByLabel("Scenario JSON").inputValue());
  expect(restored).toHaveProperty("reference_context.versions");
  expect(await receiver.getByLabel("Scenario JSON").inputValue()).toBe(recipe);
  const tampered = new URL(shared);
  const changedRecipe: unknown = JSON.parse(recipe);
  if (typeof changedRecipe !== "object" || changedRecipe === null || Array.isArray(changedRecipe))
    throw new Error("Expected a saved scenario object");
  Object.assign(changedRecipe, { name: "Changed shared recipe" });
  tampered.hash = new URLSearchParams({ recipe: JSON.stringify(changedRecipe) }).toString();
  await receiver.goto(tampered.href);
  await expect(receiver.getByRole("alert")).toContainText(/scenario/iu);
  await expect(receiver.getByLabel("Scenario JSON")).toHaveValue("");
  await context.close();
});

test("corrupt imports preserve the saved corpus and keyboard controls fit narrow screens", async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await sample(page);
  await page.getByLabel("Import corpus or fixture").setInputFiles({
    name: "broken.json",
    mimeType: "application/json",
    buffer: Buffer.from('{"schema_version":99}'),
  });
  await expect(page.getByRole("alert")).toBeVisible();
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Demonstration Act", exact: true, level: 2 }),
  ).toBeVisible();
  await page.getByLabel("Law title, ID, or provision text").focus();
  await page.keyboard.type("Demonstration");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "Demonstration Act", exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  expect(
    await page
      .locator("input, select, textarea")
      .evaluateAll((controls) =>
        controls.every(
          (control) => control.id && document.querySelector(`label[for="${control.id}"]`),
        ),
      ),
  ).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("pages-narrow.png"), fullPage: true });
});

test("official collection follows the e-Gov envelope and persists matching source metadata", async ({
  page,
}) => {
  await page.addInitScript((officialOrigin: string): void => {
    const policies: {
      cache: RequestCache;
      credentials: RequestCredentials;
      redirect: RequestRedirect;
    }[] = [];
    Object.defineProperty(window, "lawHistoryFetchPolicies", { value: policies });
    const original = window.fetch.bind(window);
    window.fetch = (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const request = new Request(input, init);
      if (new URL(request.url).origin === officialOrigin)
        policies.push({
          cache: request.cache,
          credentials: request.credentials,
          redirect: request.redirect,
        });
      return original(input, init);
    };
  }, EGOV_ORIGIN);
  const law = { law_id: "TEST_ACT", law_num: "Synthetic act", promulgation_date: "1969-01-01" };
  const revision = {
    law_revision_id: "TEST_REVISION",
    law_title: "Synthetic official envelope",
    updated: "2026-11-05T00:00:00Z",
    amendment_enforcement_date: "1970-04-01",
    amendment_enforcement_comment: "Confirmed in fixture",
    amendment_type: "1",
    mission: "New",
    repeal_status: "None",
    current_revision_status: "CurrentEnforced",
    amendment_law_id: "AMENDING_ACT",
  };
  const fixture: unknown = JSON.parse(
    await readFile(new URL("../../fixtures/demo.json", import.meta.url), "utf8"),
  );
  expect(fixture).toBeInstanceOf(Array);
  const xml =
    '<Law Era="Showa" Year="44" PromulgateMonth="1" PromulgateDay="1"><LawBody><MainProvision><Article Num="1"><Sentence>Collected fixture</Sentence></Article></MainProvision></LawBody></Law>';
  await page.route(EGOV_ROUTE, async (route): Promise<void> => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/laws"))
      await route.fulfill({
        json: { total_count: 1, laws: [{ law_info: law, revision_info: revision }] },
      });
    else if (path.includes("law_revisions"))
      await route.fulfill({ json: { law_info: law, revisions: [revision] } });
    else
      await route.fulfill({
        json: {
          law_info: law,
          revision_info: revision,
          law_full_text: Buffer.from(xml).toString("base64"),
        },
      });
  });
  await page.goto("./");
  await page.getByLabel("Official law title").fill("Synthetic");
  await page.getByRole("button", { name: "Find official laws", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Synthetic official envelope", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Collect available revisions", exact: true }).click();
  await expect(page.getByText("Saved all 1 available revisions in this browser.")).toBeVisible();
  const policies: unknown = await page.evaluate((): unknown =>
    Reflect.get(window, "lawHistoryFetchPolicies"),
  );
  expect(policies).toEqual(
    Array.from({ length: 3 }, () => ({
      cache: "no-store",
      credentials: "omit",
      redirect: "error",
    })),
  );
  await expect(
    page.getByRole("heading", { name: "Synthetic official envelope", exact: true, level: 2 }),
  ).toBeVisible();
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Synthetic official envelope", exact: true, level: 2 }),
  ).toBeVisible();
});

test("a corrupt persisted corpus can be replaced by an explicit valid import", async ({ page }) => {
  await sample(page);
  await page.evaluate(
    async (): Promise<void> =>
      new Promise((resolve, reject) => {
        const request = indexedDB.open("law-history", 1);
        request.onsuccess = (): void => {
          const db = request.result;
          const transaction = db.transaction("corpus", "readwrite");
          transaction.objectStore("corpus").put(new TextEncoder().encode("{}"), "snapshot");
          transaction.oncomplete = (): void => {
            db.close();
            resolve();
          };
          transaction.onabort = (): void => reject(transaction.error);
        };
        request.onerror = (): void => reject(request.error);
      }),
  );
  await page.reload();
  await expect(page.getByRole("alert")).toBeVisible();
  await page
    .getByLabel("Import corpus or fixture")
    .setInputFiles(new URL("../../fixtures/demo.json", import.meta.url).pathname);
  await expect(page.getByText(/Corpus imported\./u)).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Demonstration Act", exact: true, level: 2 }),
  ).toBeVisible();
});

test("ruby annotations remain separate from provision text and comparison previews", async ({
  page,
}) => {
  const fixtureSchema = z.array(z.object({ xml: z.string() }).passthrough());
  const fixture = fixtureSchema.parse(
    JSON.parse(await readFile(new URL("../../fixtures/demo.json", import.meta.url), "utf8")),
  );
  const first = fixture[0];
  if (!first) throw new Error("The source fixture is missing");
  first.xml = first.xml.replace("書面", "<Ruby>書面<Rt>しょめん</Rt></Ruby>");
  await page.goto("./?at=1969-12-31");
  await page.getByLabel("Import corpus or fixture").setInputFiles({
    name: "ruby.json",
    mimeType: "application/json",
    buffer: Buffer.from(JSON.stringify(fixture)),
  });
  await expect(page.getByText(/Corpus imported\./u)).toBeVisible();
  await page.getByRole("button", { name: "Demonstration Act", exact: true }).click();
  await page.getByText("Selected law text", { exact: true }).click();
  const law = page.getByRole("region", { name: "Demonstration Act", exact: true });
  await expect(law.locator("ruby rt")).toHaveText("しょめん");
  const preview = page.getByLabel("Provision to amend");
  await expect(preview).not.toContainText("しょめん");
});

test("collection cancellation preserves completed revisions and a later attempt completes the history", async ({
  page,
}) => {
  const law = { law_id: "STOP_ACT", law_num: "Synthetic act", promulgation_date: "1969-01-01" };
  const current = {
    law_revision_id: "STOP_CURRENT",
    law_title: "Interrupted fixture",
    updated: "2026-11-05T00:00:00Z",
    amendment_enforcement_date: "1970-04-01",
    amendment_type: "1",
    mission: "New",
    repeal_status: "None",
    current_revision_status: "CurrentEnforced",
  };
  const previous = {
    ...current,
    law_revision_id: "STOP_PREVIOUS",
    amendment_enforcement_date: "1969-01-01",
    current_revision_status: "PreviousEnforced",
  };
  const xml =
    '<Law><LawBody><MainProvision><Article Num="1"><Sentence>Interrupted collection fixture</Sentence></Article></MainProvision></LawBody></Law>';
  let stop = true;
  await page.route(EGOV_ROUTE, async (route): Promise<void> => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/laws"))
      await route.fulfill({
        json: { total_count: 1, laws: [{ law_info: law, revision_info: current }] },
      });
    else if (path.includes("law_revisions"))
      await route.fulfill({ json: { law_info: law, revisions: [previous, current] } });
    else if (path.includes("STOP_PREVIOUS") && stop) return;
    else
      await route.fulfill({
        json: {
          law_info: law,
          revision_info: path.includes("STOP_PREVIOUS") ? previous : current,
          law_full_text: Buffer.from(xml).toString("base64"),
        },
      });
  });
  await page.goto("./");
  await page.getByRole("button", { name: "Find official laws", exact: true }).click();
  const unfinished = page.waitForRequest((request) =>
    request.url().includes("law_data/STOP_PREVIOUS"),
  );
  await page.getByRole("button", { name: "Collect available revisions", exact: true }).click();
  await unfinished;
  await page.getByRole("button", { name: "Stop collection", exact: true }).click();
  await expect(
    page.getByText("Collection stopped. Completed revisions remain saved."),
  ).toBeVisible();
  await page.getByRole("button", { name: "Interrupted fixture", exact: true }).click();
  const selected = page.getByRole("region", { name: "Interrupted fixture", exact: true });
  await expect(selected.getByText(/^1 stored revisions of 2 available/u)).toBeVisible();
  await page.reload();
  await expect(selected.getByText(/^1 stored revisions of 2 available/u)).toBeVisible();
  stop = false;
  await page.getByRole("button", { name: "Find official laws", exact: true }).click();
  await page.getByRole("button", { name: "Collect available revisions", exact: true }).click();
  await expect(page.getByText("Saved all 2 available revisions in this browser.")).toBeVisible();
  await expect(selected.getByText(/^2 stored revisions of 2 available/u)).toBeVisible();
});
