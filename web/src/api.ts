import createClient from "openapi-fetch";
import { z } from "zod";
import type { components, paths } from "./api.generated";
import { browserFetch, browserMode, execute } from "./browser";

export type Model = components["schemas"];
export type LawElement = Model["Element"];
export type Scenario = Model["Scenario"];

const client = createClient<paths>({
  baseUrl: window.location.origin,
  ...(browserMode ? { fetch: browserFetch } : {}),
});
const nullableString = z.string().nullable().default(null);
const confidence = z.enum(["official", "reconstructed", "inferred", "synthetic"]);
const date = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("known"),
    date: z.string(),
    original: z.string(),
    basis: z.string(),
  }),
  z.strictObject({ kind: z.literal("uncertain"), original: z.string(), reason: z.string() }),
]);
export const provenance = z.strictObject({
  url: z.string(),
  raw_sha256: z.string(),
  media_type: z.string(),
  attribution: z.string(),
  terms_url: z.string(),
  confidence,
  locator: z.string(),
}) satisfies z.ZodType<Model["Provenance"]>;

const element: z.ZodType<LawElement> = z.lazy(() =>
  z.strictObject({
    tag: z.string(),
    attributes: z.record(z.string(), z.string()).default({}),
    content: z
      .array(
        z.discriminatedUnion("kind", [
          z.object({ kind: z.literal("text"), value: z.string() }),
          z.object({ kind: z.literal("element"), value: element }),
        ]),
      )
      .default([]),
  }),
);

const diagnostic = z.object({
  code: z.enum([
    "unknown_date",
    "ambiguous_order",
    "missing_source",
    "unsupported_syntax",
    "wrong_base",
    "original_mismatch",
    "conflict",
    "official_mismatch",
    "unresolved_reference",
    "coverage_gap",
    "invalid_input",
  ]),
  message: z.string(),
  source: z.string(),
  locator: z.string(),
});
const coverage = z.object({
  law_id: z.string(),
  available_revisions: z.number().nullable().default(null),
  stored_revisions: z.number(),
  earliest_known: nullableString,
  enactment_known: z.boolean(),
  all_available_stored: z.boolean(),
  last_collected: nullableString,
  diagnostics: z.array(diagnostic),
});
const revision = z.object({
  schema_version: z.literal(1),
  law_id: z.string(),
  revision_id: z.string(),
  title: z.string(),
  promulgated: date,
  amendment_promulgated: date.nullable().default(null),
  effective: date,
  repeal: nullableString,
  termination: z
    .object({ status: z.enum(["repealed", "expired", "suspended", "lost_effectiveness"]), date })
    .nullable()
    .default(null),
  sequence: z.number().nullable().default(null),
  kind: z.enum(["enactment", "amendment", "correction", "repeal", "consolidated"]),
  amendment_law_id: nullableString,
  original_enactment: z.boolean(),
  tree: element,
  source: provenance,
  metadata_source: provenance.nullable().default(null),
}) satisfies z.ZodType<Model["Revision"]>;
const stored = z.object({ version: z.string(), observed_at: z.string(), revision });
const selection = z.object({ selected: stored, coverage, diagnostics: z.array(diagnostic) });
const hit = z.object({
  law_id: z.string(),
  title: z.string(),
  version: z.string(),
  effective: date,
  coverage,
});
const located = z.object({ path: z.string(), element });
const diff = z.object({
  from: z.string(),
  to: z.string(),
  diagnostics: z.array(diagnostic),
  changes: z.array(
    z.object({
      kind: z.enum(["added", "removed", "changed", "moved", "renumbered"]),
      before: located.nullable().default(null),
      after: located.nullable().default(null),
      evidence: provenance.nullable().default(null),
    }),
  ),
});
const position = z.object({ law_id: z.string(), version: z.string(), path: z.string() });
const reference = z.object({
  from: position,
  to: position.nullable().default(null),
  text: z.string(),
  kind: z.enum(["provision", "definition", "delegation"]),
  confidence,
  diagnostics: z.array(diagnostic),
});
const impact = z.object({
  context_versions: z.record(z.string(), z.string()),
  direct: z.array(position),
  review: z.array(z.object({ position, via: z.array(reference) })),
  broken: z.array(reference),
  diagnostics: z.array(diagnostic),
});
const operation = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("replace_text"),
    law_id: z.string(),
    path: z.string(),
    old: z.string(),
    new: z.string(),
  }),
  z.strictObject({
    kind: z.literal("replace"),
    law_id: z.string(),
    path: z.string(),
    expected: element,
    replacement: element,
  }),
  z.strictObject({
    kind: z.literal("insert"),
    law_id: z.string(),
    parent: z.string(),
    before: nullableString,
    element,
  }),
  z.strictObject({
    kind: z.literal("delete"),
    law_id: z.string(),
    path: z.string(),
    expected: element,
  }),
  z.strictObject({
    kind: z.literal("move"),
    law_id: z.string(),
    path: z.string(),
    expected: element,
    parent: z.string(),
    before: nullableString,
  }),
  z.strictObject({
    kind: z.literal("renumber"),
    law_id: z.string(),
    path: z.string(),
    from: z.string(),
    to: z.string(),
    title: z.string(),
  }),
  z.strictObject({ kind: z.literal("repeal"), law_id: z.string() }),
]);
const sourcedOperation = z.strictObject({ operation, evidence: provenance });
export const scenarioSchema = z.strictObject({
  schema_version: z.literal(1),
  name: z.string(),
  bases: z.record(z.string(), z.string()),
  at: z.string(),
  known_at: nullableString,
  reference_context: z
    .strictObject({
      versions: z.record(z.string(), z.string()),
      diagnostics: z.array(diagnostic),
    })
    .nullable()
    .default(null),
  steps: z.array(
    z.strictObject({
      id: z.string(),
      effective: date,
      after: z.array(z.string()).default([]),
      operations: z.array(sourcedOperation).default([]),
      adjustments: z
        .array(
          z.strictObject({
            target_step: z.string(),
            expected_operations_sha256: z.string(),
            replacement: z.array(sourcedOperation),
          }),
        )
        .default([]),
    }),
  ),
}) satisfies z.ZodType<Scenario>;
const simulation = z.object({
  scenario_id: z.string(),
  applied_steps: z.array(z.string()),
  results: z.record(z.string(), revision),
  diffs: z.array(diff),
  diagnostics: z.array(diagnostic),
  impact,
});
const billId = z.object({
  kind: z.enum([
    "cabinet",
    "representatives",
    "councillors",
    "budget",
    "treaty",
    "approval",
    "consent",
    "accounts",
    "resolution",
    "rules",
    "regulations",
  ]),
  house: z.enum(["representatives", "councillors"]),
  session: z.number(),
  number: z.number(),
});
const parliament = z.object({
  bills: z.array(
    z.object({
      schema_version: z.number(),
      id: billId,
      title: z.string(),
      enacted_law_ids: z.array(z.string()),
      submitters: z.array(z.string()),
      reason: z.string(),
      speeches: z.array(z.object({ speech_id: z.string(), confidence, basis: z.string() })),
      source: provenance,
    }),
  ),
  speeches: z.array(
    z.object({
      speech_id: z.string(),
      meeting_id: z.string(),
      session: z.number(),
      house: z.string(),
      meeting: z.string(),
      date: z.string(),
      speaker: z.string(),
      text: z.string(),
      url: z.string(),
      source: provenance,
    }),
  ),
});
const status = z.object({
  catalog: z
    .object({
      schema_version: z.number(),
      collected_at: z.string(),
      source_url: z.string(),
      reported_count: z.number(),
      law_ids: z.array(z.string()),
      complete: z.boolean(),
    })
    .nullable()
    .default(null),
  laws: z.array(coverage),
  diagnostics: z.array(diagnostic),
});

async function checked<T>(
  request: Promise<{ response: Response; data?: unknown; error?: unknown }>,
  schema: z.ZodType<T>,
): Promise<T> {
  const result = await request;
  if (!result.response.ok) {
    const error = z.object({ error: z.string() }).safeParse(result.error);
    throw new Error(
      error.success ? error.data.error : `Request failed (${result.response.status})`,
    );
  }
  return schema.parse(result.data);
}

export interface Temporal {
  at?: string;
  known_at?: string;
  version?: string;
}
export function loadSearch(
  q: string,
  time: Temporal,
  signal: AbortSignal,
): Promise<Model["SearchResponse"]> {
  return checked(
    client.GET("/api/search", { params: { query: { q, ...time } }, signal }),
    z.object({ hits: z.array(hit), diagnostics: z.array(diagnostic) }),
  );
}
export function loadLaw(
  law: string,
  time: Temporal,
  signal: AbortSignal,
): Promise<Model["Selection"]> {
  return checked(
    client.GET("/api/laws/{law_id}", { params: { path: { law_id: law }, query: time }, signal }),
    selection,
  );
}
export function loadHistory(
  law: string,
  time: Temporal,
  signal: AbortSignal,
): Promise<Model["StoredRevision"][]> {
  return checked(
    client.GET("/api/laws/{law_id}/log", {
      params: { path: { law_id: law }, query: time },
      signal,
    }),
    z.array(stored),
  );
}
export function loadRefs(
  law: string,
  time: Temporal,
  signal: AbortSignal,
): Promise<Model["ReferenceReport"]> {
  return checked(
    client.GET("/api/laws/{law_id}/refs", {
      params: { path: { law_id: law }, query: time },
      signal,
    }),
    z.object({
      references: z.array(reference),
      context_versions: z.record(z.string(), z.string()),
      diagnostics: z.array(diagnostic),
    }),
  );
}
export function loadDiff(from: string, to: string, signal: AbortSignal): Promise<Model["Diff"]> {
  return checked(client.GET("/api/diff", { params: { query: { from, to } }, signal }), diff);
}
export function loadImpact(
  from: string,
  to: string,
  time: Temporal,
  signal: AbortSignal,
): Promise<Model["Impact"]> {
  return checked(
    client.GET("/api/impact", { params: { query: { from, to, ...time } }, signal }),
    impact,
  );
}
export function simulateScenario(scenario: Scenario): Promise<Model["Simulation"]> {
  return checked(client.POST("/api/simulate", { body: scenario }), simulation);
}
export async function saveScenario(scenario: Scenario): Promise<string> {
  const result = await checked(
    client.POST("/api/scenarios", { body: scenario }),
    z.object({ id: z.string() }),
  );
  if (browserMode) {
    const saved = scenarioSchema.parse(
      await execute({ operation: "load_scenario", id: result.id }),
    );
    const fragment = new URLSearchParams({ recipe: JSON.stringify(saved) });
    window.history.replaceState(
      null,
      "",
      `${window.location.pathname}${window.location.search}#${fragment}`,
    );
  }
  return result.id;
}
export function loadScenario(id: string, signal: AbortSignal): Promise<Scenario> {
  return checked(
    client.GET("/api/scenarios/{id}", { params: { path: { id } }, signal }),
    scenarioSchema,
  );
}
export function loadParliament(
  law: string,
  time: Temporal,
  signal: AbortSignal,
): Promise<Model["ParliamentaryContext"]> {
  return checked(
    client.GET("/api/laws/{law_id}/parliament", {
      params: { path: { law_id: law }, query: time.known_at ? { known_at: time.known_at } : {} },
      signal,
    }),
    parliament,
  );
}
export function loadStatus(signal: AbortSignal): Promise<Model["Status"]> {
  return checked(client.GET("/api/status", { signal }), status);
}
