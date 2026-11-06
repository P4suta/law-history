import { z } from "zod";

export const browserMode = import.meta.env.MODE === "pages";
const EGOV = "https://laws.e-gov.go.jp/api/2";
const MAX_SOURCE_BYTES = 64 * 1024 * 1024;
const MAX_IMPORT_BYTES = 256 * 1024 * 1024;
const HTTP_CLIENT_ERROR = 400;
const FRAGMENT_PREFIX_LENGTH = 1;
const SOURCE_TIMEOUT_MS = 60_000;
const REVISION_INTERVAL_MS = 250;
const DOWNLOAD_REVOKE_DELAY_MS = 1000;
const workerReply = z.discriminatedUnion("ok", [
  z.object({ id: z.number().int(), ok: z.literal(true), value: z.unknown() }),
  z.object({ id: z.number().int(), ok: z.literal(false), error: z.string() }),
]);
const engineReply = z.object({ status: z.number().int(), body: z.unknown() });
type Pending = { resolve: (value: unknown) => void; reject: (error: Error) => void };
const pending = new Map<number, Pending>();
let worker: Worker | undefined;
let nextId = 0;

function connection(): Worker {
  if (worker) return worker;
  worker = new Worker(new URL("./history.worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (event: MessageEvent<unknown>): void => {
    const parsed = workerReply.safeParse(event.data);
    if (!parsed.success) {
      failAll(new Error("The history worker returned an invalid response"));
      return;
    }
    const message = parsed.data;
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    if (message.ok) request.resolve(message.value);
    else request.reject(new Error(message.error));
  };
  worker.onerror = (event: ErrorEvent): void =>
    failAll(new Error(event.message || "The history worker failed"));
  worker.onmessageerror = (): void =>
    failAll(new Error("The history worker response could not be read"));
  return worker;
}

function failAll(error: Error): void {
  for (const request of pending.values()) request.reject(error);
  pending.clear();
  worker?.terminate();
  worker = undefined;
}

function send(
  message:
    | { kind: "execute"; payload: string }
    | { kind: "snapshot" }
    | { kind: "restore"; payload: Uint8Array },
): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    const channel = connection();
    pending.set(id, { resolve, reject });
    channel.postMessage({ id, ...message });
  });
}

export async function execute(command: Record<string, unknown>): Promise<unknown> {
  const reply = engineReply.parse(
    await send({ kind: "execute", payload: JSON.stringify(command) }),
  );
  if (reply.status >= HTTP_CLIENT_ERROR) {
    const error = z.object({ error: z.string() }).parse(reply.body);
    throw new Error(error.error);
  }
  return reply.body;
}

export async function browserFetch(
  input: RequestInfo | URL,
  init?: RequestInit,
): Promise<Response> {
  const request = new Request(input, init);
  request.signal.throwIfAborted();
  const url = new URL(request.url);
  const query: Record<string, string> = {};
  for (const key of ["at", "known_at", "version"]) {
    const value = url.searchParams.get(key);
    if (value) query[key] = value;
  }
  const [root, resource, identifier, action] = url.pathname.split("/").filter(Boolean);
  let command: Record<string, unknown>;
  if (root !== "api") throw new Error("Unknown browser request");
  switch (resource) {
    case "search":
      command = { operation: "search", q: url.searchParams.get("q") ?? "", query };
      break;
    case "status":
      command = { operation: "status" };
      break;
    case "diff":
    case "impact":
      command = {
        operation: resource,
        from: url.searchParams.get("from"),
        to: url.searchParams.get("to"),
        ...(resource === "impact" ? { query } : {}),
      };
      break;
    case "laws":
      command = {
        operation: action === "log" ? "history" : (action ?? "show"),
        law: decodeURIComponent(identifier ?? ""),
        query,
      };
      break;
    case "simulate":
      command = { operation: "simulate", scenario: await request.json() };
      break;
    case "scenarios": {
      if (request.method === "POST")
        command = { operation: "save_scenario", scenario: await request.json() };
      else {
        const shared = new URLSearchParams(window.location.hash.slice(FRAGMENT_PREFIX_LENGTH)).get(
          "recipe",
        );
        const id = decodeURIComponent(identifier ?? "");
        command = shared
          ? { operation: "shared_scenario", id, scenario: JSON.parse(shared) }
          : { operation: "load_scenario", id };
      }
      break;
    }
    default:
      throw new Error("Unsupported browser request");
  }
  const reply = engineReply.parse(
    await send({ kind: "execute", payload: JSON.stringify(command) }),
  );
  request.signal.throwIfAborted();
  return Response.json(reply.body, { status: reply.status });
}

export async function readSource(url: string, signal: AbortSignal): Promise<Uint8Array> {
  const response = await fetch(url, {
    signal: AbortSignal.any([signal, AbortSignal.timeout(SOURCE_TIMEOUT_MS)]),
    credentials: "omit",
    redirect: "error",
    cache: "no-store",
  });
  if (!response.ok || !response.body) throw new Error(`Source request failed (${response.status})`);
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  try {
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) break;
      size += chunk.value.byteLength;
      if (size > MAX_SOURCE_BYTES) throw new Error("Source exceeds the 64 MiB input limit");
      chunks.push(chunk.value);
    }
  } finally {
    await reader.cancel();
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

const catalogSchema = z.object({
  total_count: z.number().int().nonnegative(),
  laws: z.array(
    z.object({
      law_info: z.object({
        law_id: z.string().regex(/^[A-Za-z0-9_-]{1,200}$/u),
        law_num: z.string(),
      }),
      revision_info: z.object({ law_title: z.string() }),
    }),
  ),
});
export type OfficialLaw = z.infer<typeof catalogSchema>["laws"][number];
export async function findOfficial(
  title: string,
  signal: AbortSignal,
): Promise<{ total: number; laws: OfficialLaw[] }> {
  const query = new URLSearchParams({ law_title: title, limit: "20" });
  const source = await readSource(`${EGOV}/laws?${query}`, signal);
  const catalog = catalogSchema.parse(
    await execute({ operation: "catalog", bytes: Array.from(source) }),
  );
  return { total: catalog.total_count, laws: catalog.laws };
}

const inventorySchema = z.object({
  revision_ids: z.array(z.string()),
  source_digest: z.string(),
  current_revision_id: z.string().nullable().optional(),
});
export async function collectLaw(
  law: string,
  signal: AbortSignal,
  progress: (message: string) => void,
): Promise<void> {
  progress("Fetching the official revision inventory…");
  const source = await readSource(`${EGOV}/law_revisions/${encodeURIComponent(law)}`, signal);
  signal.throwIfAborted();
  const inventory = inventorySchema.parse(
    await execute({ operation: "collect_history", law, bytes: Array.from(source) }),
  );
  const revisions = [...inventory.revision_ids].sort(
    (a, b) =>
      Number(b === inventory.current_revision_id) - Number(a === inventory.current_revision_id),
  );
  const failures: string[] = [];
  let completed = 0;
  for (const revision of revisions) {
    signal.throwIfAborted();
    progress(`Collecting revision ${completed + 1} of ${revisions.length}…`);
    try {
      const bytes = await readSource(
        `${EGOV}/law_data/${encodeURIComponent(revision)}?law_full_text_format=xml`,
        signal,
      );
      signal.throwIfAborted();
      await execute({
        operation: "collect_revision",
        law,
        revision,
        history: inventory.source_digest,
        bytes: Array.from(bytes),
      });
    } catch (failure) {
      signal.throwIfAborted();
      failures.push(`${revision}: ${failure instanceof Error ? failure.message : String(failure)}`);
    }
    completed += 1;
    if (completed < revisions.length)
      await new Promise<void>((resolve) => window.setTimeout(resolve, REVISION_INTERVAL_MS));
  }
  if (failures.length)
    throw new Error(
      `${completed - failures.length} of ${completed} revisions saved. ${failures.join("; ")}`,
    );
  progress(`Saved all ${completed} available revisions in this browser.`);
}

export async function importCorpus(file: File): Promise<void> {
  if (file.size > MAX_IMPORT_BYTES) throw new Error("Import exceeds the 256 MiB input limit");
  const bytes = new Uint8Array(await file.arrayBuffer());
  const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes).trimStart();
  if (text.startsWith("[")) await execute({ operation: "import", bytes: Array.from(bytes) });
  else await send({ kind: "restore", payload: bytes });
}

export async function exportCorpus(): Promise<void> {
  const bytes = z.instanceof(Uint8Array).parse(await send({ kind: "snapshot" }));
  const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "application/json" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = "law-history-corpus.json";
  anchor.click();
  window.setTimeout(() => URL.revokeObjectURL(url), DOWNLOAD_REVOKE_DELAY_MS);
}
