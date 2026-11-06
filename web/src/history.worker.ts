import { z } from "zod";
import init, { Engine } from "./wasm/law_browser";

const HTTP_CLIENT_ERROR = 400;

const requestSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    id: z.number().int().positive(),
    kind: z.literal("execute"),
    payload: z.string(),
  }),
  z.strictObject({ id: z.number().int().positive(), kind: z.literal("snapshot") }),
  z.strictObject({
    id: z.number().int().positive(),
    kind: z.literal("restore"),
    payload: z.instanceof(Uint8Array),
  }),
]);
const replySchema = z.object({ status: z.number().int(), body: z.unknown() });
const effectSchema = z.enum(["read", "write", "import", "invalid"]);

function database(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open("law-history", 1);
    request.onupgradeneeded = (): void => {
      request.result.createObjectStore("corpus");
    };
    request.onsuccess = (): void => resolve(request.result);
    request.onerror = (): void =>
      reject(request.error ?? new Error("Browser storage is unavailable"));
    request.onblocked = (): void =>
      reject(new Error("Close other law-history tabs to upgrade browser storage"));
  });
}

async function stored(db: IDBDatabase): Promise<Uint8Array | undefined> {
  return new Promise((resolve, reject) => {
    const transaction = db.transaction("corpus", "readonly");
    const request = transaction.objectStore("corpus").get("snapshot");
    let value: unknown;
    request.onsuccess = (): void => {
      value = request.result;
    };
    transaction.oncomplete = (): void => {
      if (value === undefined || value instanceof Uint8Array) resolve(value);
      else reject(new Error("The saved browser corpus has an unsupported format"));
    };
    transaction.onabort = (): void =>
      reject(transaction.error ?? new Error("Browser corpus read failed"));
  });
}

async function persist(db: IDBDatabase, bytes: Uint8Array): Promise<void> {
  return new Promise((resolve, reject) => {
    const transaction = db.transaction("corpus", "readwrite");
    transaction.objectStore("corpus").put(bytes, "snapshot");
    transaction.oncomplete = (): void => resolve();
    transaction.onabort = (): void =>
      reject(transaction.error ?? new Error("Browser storage is full or unavailable"));
  });
}

interface Runtime {
  engine: Engine;
  db: IDBDatabase;
  loadError: string | null;
}

async function initialize(): Promise<Runtime> {
  await init();
  const engine = new Engine();
  const db = await database();
  let loadError: string | null = null;
  try {
    const bytes = await stored(db);
    if (bytes) engine.restore(bytes);
  } catch (failure) {
    loadError = `The saved corpus could not be loaded. Import a valid corpus to replace it. ${failure instanceof Error ? failure.message : String(failure)}`;
  }
  return { engine, db, loadError };
}

const ready = initialize();
let pending = Promise.resolve();
self.addEventListener("message", (event: MessageEvent<unknown>): void => {
  pending = pending.then(async (): Promise<void> => {
    const parsed = requestSchema.safeParse(event.data);
    if (!parsed.success) return;
    const request = parsed.data;
    try {
      const state = await ready;
      const { engine, db } = state;
      if (request.kind === "snapshot") {
        if (state.loadError) throw new Error(state.loadError);
        self.postMessage({ id: request.id, ok: true, value: engine.snapshot() });
        return;
      }
      const effect =
        request.kind === "restore"
          ? "import"
          : effectSchema.parse(Engine.request_effect(request.payload));
      const before = effect === "write" || effect === "import" ? engine.snapshot() : undefined;
      try {
        if (request.kind === "restore") {
          engine.restore(request.payload);
          await persist(db, engine.snapshot());
          state.loadError = null;
          self.postMessage({ id: request.id, ok: true, value: null });
        } else {
          if (state.loadError && effect !== "import") throw new Error(state.loadError);
          const reply = replySchema.parse(
            JSON.parse(engine.execute(request.payload, new Date().toISOString())),
          );
          if (reply.status < HTTP_CLIENT_ERROR && before) {
            await persist(db, engine.snapshot());
            state.loadError = null;
          }
          self.postMessage({ id: request.id, ok: true, value: reply });
        }
      } catch (failure) {
        if (before) engine.restore(before);
        throw failure;
      }
    } catch (failure) {
      self.postMessage({
        id: request.id,
        ok: false,
        error: failure instanceof Error ? failure.message : String(failure),
      });
    }
  });
});
