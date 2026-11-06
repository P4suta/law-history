import { type FormEvent, type ReactElement, useEffect, useRef, useState } from "react";
import {
  collectLaw,
  execute,
  exportCorpus,
  findOfficial,
  importCorpus,
  type OfficialLaw,
  readSource,
} from "./browser";

export function Corpus({ changed }: { changed: (law?: string) => void }): ReactElement {
  const [title, setTitle] = useState("日本国憲法");
  const [laws, setLaws] = useState<OfficialLaw[]>([]);
  const [total, setTotal] = useState<number | null>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => (): void => controller.current?.abort(), []);

  async function perform(operation: (signal: AbortSignal) => Promise<void>): Promise<void> {
    controller.current?.abort();
    const active = new AbortController();
    controller.current = active;
    setBusy(true);
    setError("");
    setMessage("");
    try {
      await operation(active.signal);
    } catch (failure) {
      if (active.signal.aborted)
        setMessage("Collection stopped. Completed revisions remain saved.");
      else setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      if (controller.current === active) {
        setBusy(false);
        controller.current = null;
      }
      changed();
    }
  }

  async function search(event: FormEvent<HTMLFormElement>): Promise<void> {
    event.preventDefault();
    await perform(async (signal): Promise<void> => {
      const result = await findOfficial(title, signal);
      setLaws(result.laws);
      setTotal(result.total);
    });
  }

  function collect(law: string): void {
    void perform(async (signal): Promise<void> => {
      await collectLaw(law, signal, setMessage);
      changed(law);
    });
  }

  return (
    <section aria-labelledby="corpus-heading">
      <h2 id="corpus-heading">Collect official law histories</h2>
      <p>
        Law histories are fetched from e-Gov and saved in this browser. Search, comparisons, and
        simulations use the collected corpus. Browser storage may be cleared by the browser; export
        the corpus to keep a copy.
      </p>
      <p>
        Snapshot imports replace the browser corpus. Export the current corpus before importing a
        snapshot from another browser.
      </p>
      <p>Corpora must fit in a 256 MiB export file. Use the CLI for larger collections.</p>
      <form
        onSubmit={(event): void => {
          void search(event);
        }}
        className="search-form"
      >
        <div>
          <label htmlFor="official-title">Official law title</label>
          <input
            id="official-title"
            value={title}
            onChange={(event): void => setTitle(event.target.value)}
            required
          />
        </div>
        <button type="submit" disabled={busy}>
          Find official laws
        </button>
        <button type="button" disabled={busy} onClick={(): void => collect("321CONSTITUTION")}>
          Collect the Constitution of Japan
        </button>
      </form>
      {total !== null && <p>{total} official matches; showing up to 20.</p>}
      {laws.map((law) => (
        <article key={law.law_info.law_id}>
          <h3 lang="ja">{law.revision_info.law_title}</h3>
          <p>
            {law.law_info.law_num} · <code>{law.law_info.law_id}</code>
          </p>
          <button type="button" disabled={busy} onClick={(): void => collect(law.law_info.law_id)}>
            Collect available revisions
          </button>
        </article>
      ))}
      <div className="actions">
        <button
          type="button"
          disabled={busy}
          onClick={(): void => {
            void perform(async (signal): Promise<void> => {
              const bytes = await readSource(
                new URL("demo.json", window.location.href).href,
                signal,
              );
              await execute({ operation: "import", bytes: Array.from(bytes) });
              setMessage("Synthetic sample loaded. The sample is not an official law.");
              changed("DEMO_ACT");
            });
          }}
        >
          Load synthetic sample
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={(): void => {
            void perform(async (): Promise<void> => {
              await exportCorpus();
              setMessage("Corpus exported with sources, versions, and saved scenarios.");
            });
          }}
        >
          Export corpus
        </button>
        <label htmlFor="corpus-file">Import corpus or fixture</label>
        <input
          id="corpus-file"
          type="file"
          accept="application/json,.json"
          disabled={busy}
          onChange={(event): void => {
            const file = event.target.files?.[0];
            if (file)
              void perform(async (): Promise<void> => {
                await importCorpus(file);
                setMessage("Corpus imported. Snapshot imports replace the browser corpus.");
              });
            event.target.value = "";
          }}
        />
        {busy && (
          <button type="button" onClick={(): void => controller.current?.abort()}>
            Stop collection
          </button>
        )}
      </div>
      <p role="status">{message}</p>
      {error && <p role="alert">{error}</p>}
      <p>
        <a href="https://laws.e-gov.go.jp/terms">e-Gov terms of use</a> ·{" "}
        <a href="https://github.com/P4suta/law-history">Source code</a>
      </p>
    </section>
  );
}
