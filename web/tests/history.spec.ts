import { expect, type Page, test } from "@playwright/test";

const HTTP_UNPROCESSABLE_ENTITY = 422;

function paletteViolations(source: string | null): string[] {
  const failures: string[] = [];
  const colors = new Set([
    "#000",
    "#fff",
    "black",
    "white",
    "rgb(0, 0, 0)",
    "rgb(255, 255, 255)",
    "currentcolor",
  ]);
  function inspect(rules: CSSRuleList): void {
    for (const rule of rules) {
      if (rule instanceof CSSStyleRule) {
        for (const property of rule.style) {
          const value = rule.style.getPropertyValue(property).trim().toLowerCase();
          if (
            (property === "color" || property.endsWith("-color") || property === "background") &&
            !colors.has(value)
          )
            failures.push(`${property}: ${value}`);
          if (
            (property === "opacity" && value !== "1") ||
            (/shadow|filter|blend/u.test(property) && value !== "none" && value !== "normal") ||
            /gradient|rgba\(/u.test(value)
          )
            failures.push(`${property}: ${value}`);
        }
      } else if (rule instanceof CSSGroupingRule) inspect(rule.cssRules);
    }
  }
  if (source === null) {
    for (const sheet of document.styleSheets) inspect(sheet.cssRules);
  } else {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(source);
    inspect(sheet.cssRules);
  }
  return failures;
}

async function selectLaw(page: Page, at = ""): Promise<void> {
  await page.goto(at ? `/?at=${at}` : "/");
  await page.getByRole("button", { name: "Demonstration Act", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Demonstration Act", exact: true, level: 2 }),
  ).toBeVisible();
}

test("enforcement, knowledge cutoff, comparison, references, and URL restoration", async ({
  page,
}) => {
  await selectLaw(page);
  await page.getByRole("button", { name: "Compare", exact: true }).click();
  await expect(page.getByText("申請は書面で行う。", { exact: true })).toBeVisible();
  await expect(page.getByText("申請は電磁的記録で行う。", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Review through references" })).toBeVisible();
  expect(new URL(page.url()).searchParams.get("from")).toHaveLength(64);
  expect(new URL(page.url()).searchParams.get("to")).toHaveLength(64);
  await page.reload();
  await expect(page.getByText("申請は書面で行う。", { exact: true })).toBeVisible();
  await page.getByLabel("Known at (RFC 3339)").fill("2026-01-01T12:00:00Z");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await page.getByRole("button", { name: "Demonstration Act", exact: true }).click();
  await page.getByText("Selected law text", { exact: true }).click();
  await expect(page.getByText("第一条申請は書面で行う。", { exact: true })).toBeVisible();
  await page.screenshot({ path: "test-results/history-desktop.png", fullPage: true });
});

test("revision details preserve and display complementary metadata provenance", async ({
  page,
}, testInfo) => {
  const metadataUrl = "https://example.invalid/metadata";
  let includeMetadata = false;
  await page.route("**/api/laws/DEMO_ACT?**", async (route) => {
    const include = includeMetadata;
    const response = await route.fetch();
    const selection: unknown = await response.json();
    if (typeof selection !== "object" || selection === null || !("selected" in selection))
      throw new Error("Expected a revision selection");
    const selected = selection.selected;
    if (typeof selected !== "object" || selected === null || !("revision" in selected))
      throw new Error("Expected a selected revision");
    const revision = selected.revision;
    if (typeof revision !== "object" || revision === null || !("source" in revision))
      throw new Error("Expected source provenance");
    const source = revision.source;
    if (typeof source !== "object" || source === null) throw new Error("Expected source fields");
    if (include) Object.assign(revision, { metadata_source: { ...source, url: metadataUrl } });
    await route.fulfill({ response, json: selection });
  });
  await selectLaw(page);
  await page.screenshot({ path: testInfo.outputPath("sources-before.png") });
  includeMetadata = true;
  await selectLaw(page);
  await expect(
    page.getByRole("link", { name: "Revision metadata source", exact: true }),
  ).toHaveAttribute("href", metadataUrl);
  await page.screenshot({ path: testInfo.outputPath("sources-after.png") });
});

test("selection decoders accept every declared producer enum variant", async ({
  page,
  request,
}, testInfo) => {
  function isObject(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }
  function object(value: unknown): Record<string, unknown> {
    if (!isObject(value)) throw new Error("Expected a JSON object");
    return value;
  }
  const response = await request.get("/api/openapi.json");
  const specification: unknown = await response.json();
  const definitions = object(object(object(specification).components).schemas);
  let probe: { schema: string; value: string; applied: boolean } | null = null;
  await page.route("**/api/laws/DEMO_ACT?**", async (route) => {
    const active = probe;
    const response = await route.fetch();
    const payload: unknown = await response.json();
    const revision = object(object(object(payload).selected).revision);
    if (active?.schema === "RevisionKind") revision.kind = active.value;
    if (active?.schema === "Confidence") object(revision.source).confidence = active.value;
    if (active?.schema === "TerminationStatus")
      revision.termination = {
        status: active.value,
        date: {
          kind: "uncertain",
          original: "Fixture end date",
          reason: "The fixture supplies no established date",
        },
      };
    if (active) active.applied = true;
    await route.fulfill({ response, json: payload });
  });
  for (const schema of ["RevisionKind", "Confidence", "TerminationStatus"]) {
    const values = object(definitions[schema]).enum;
    if (!Array.isArray(values) || values.length === 0)
      throw new Error("Expected nonempty producer variants");
    for (const value of values) {
      if (typeof value !== "string") throw new Error("Expected a string variant");
      const current = { schema, value, applied: false };
      probe = current;
      await selectLaw(page);
      expect(current.applied).toBe(true);
      if (schema === "TerminationStatus") {
        const evidence = page.getByText("Termination evidence:", { exact: false });
        await expect(evidence).toContainText(value);
        await expect(evidence).toContainText("Uncertain: Fixture end date");
        if (value === "repealed")
          await page.screenshot({ path: testInfo.outputPath("undated-repeal.png") });
      }
    }
  }
});

test("scenario simulation and explicit sharing preserve pinned inputs", async ({ page }) => {
  await selectLaw(page, "1969-12-31");
  await page.getByLabel("Original text", { exact: true }).fill("書面");
  await page.getByLabel("Replacement text", { exact: true }).fill("電磁的記録");
  await page.getByRole("button", { name: "Create scenario from selected revision" }).click();
  const editor = page.getByLabel("Scenario JSON");
  const initialText = await editor.inputValue();
  const initial: unknown = JSON.parse(initialText);
  expect(initial).toHaveProperty("bases.DEMO_ACT");
  for (const candidate of [
    initialText.replace('"known_at"', '"knwon_at"'),
    initialText.replace('"after"', '"depends_on"'),
    initialText.replace('"operation": {', '"operation": {"ignore_conflicts": true,'),
  ]) {
    expect(candidate).not.toBe(initialText);
    const data: unknown = JSON.parse(candidate);
    const refused = await page.request.post("/api/simulate", { data });
    expect(refused.status()).toBe(HTTP_UNPROCESSABLE_ENTITY);
    await editor.fill(candidate);
    await page.getByRole("button", { name: "Simulate", exact: true }).click();
    await expect(page.getByRole("alert")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Simulation result" })).toBeHidden();
  }
  await editor.fill(initialText);
  await page.getByRole("button", { name: "Simulate", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Simulation result" })).toBeVisible();
  expect(new URL(page.url()).searchParams.has("scenario")).toBe(false);
  await page.getByRole("button", { name: "Save scenario for sharing" }).click();
  await expect(page.getByText(/Scenario saved\./u)).toBeVisible();
  expect(new URL(page.url()).searchParams.get("scenario")).toHaveLength(64);
  await page.reload();
  await expect(editor).toHaveValue(/Proposed amendment/u);
  const pinned: unknown = JSON.parse(await editor.inputValue());
  expect(pinned).toHaveProperty("reference_context.versions");
  await editor.fill("{}");
  await page.getByRole("button", { name: "Simulate", exact: true }).click();
  await expect(page.getByRole("alert")).toBeVisible();
});

test("keyboard search, narrow layout, and form labels", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await page.getByLabel("Law title, ID, or provision text").focus();
  await page.keyboard.type("Demonstration");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "Demonstration Act", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Demonstration Act", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Compare revisions" })).toBeVisible();
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
  await page.screenshot({ path: "test-results/history-narrow.png", fullPage: true });
});

test("all authored CSS rules and computed control states use black and white", async ({ page }) => {
  await page.goto("/");
  const violations = await page.evaluate(paletteViolations, null);
  expect(violations).toEqual([]);
  await page.getByRole("button", { name: "Search", exact: true }).hover();
  await expect(page.getByRole("button", { name: "Search", exact: true })).toHaveCSS(
    "background-color",
    "rgb(0, 0, 0)",
  );
  await page.getByLabel("Law title, ID, or provision text").focus();
  await expect(page.getByLabel("Law title, ID, or provision text")).toHaveCSS(
    "outline-color",
    "rgb(0, 0, 0)",
  );
});

test("palette inspection rejects planted gray, alpha, and compositing rules", async ({ page }) => {
  await page.goto("/");
  for (const source of [
    "p { color: #777; }",
    "p { color: rgba(0, 0, 0, .5); }",
    "p { opacity: .5; }",
    "p { background-image: linear-gradient(black, white); }",
    "p { box-shadow: 0 0 1px black; }",
    "p { filter: blur(1px); }",
    "p { mix-blend-mode: multiply; }",
    "@media (min-width: 1px) { p { color: gray; } }",
  ]) {
    expect(await page.evaluate(paletteViolations, source), source).not.toEqual([]);
  }
  expect(
    await page.evaluate(
      paletteViolations,
      "p { color: black; background-color: white; opacity: 1; border: 1px solid black; }",
    ),
  ).toEqual([]);
});

test("API rejects invalid dates and exposes the generated contract", async ({ request }) => {
  const invalid = await request.get("/api/search?at=1900-02-29");
  expect(invalid.status()).toBe(400);
  const schema = await request.get("/api/openapi.json");
  expect(schema.ok()).toBe(true);
  const document: unknown = await schema.json();
  expect(document).toHaveProperty("paths./api/simulate.post");
});
