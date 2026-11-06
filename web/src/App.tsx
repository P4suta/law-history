import { type FormEvent, type ReactElement, useEffect, useState } from "react";
import {
  type LawElement,
  loadDiff,
  loadHistory,
  loadImpact,
  loadLaw,
  loadParliament,
  loadRefs,
  loadScenario,
  loadSearch,
  loadStatus,
  type Model,
  type Scenario,
  saveScenario,
  scenarioSchema,
  simulateScenario,
  type Temporal,
} from "./api";

function text(node: LawElement): string {
  return (node.content ?? [])
    .map((part) => (part.kind === "text" ? part.value : text(part.value)))
    .join("");
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Request failed";
}
function dateText(date: Model["LegalDate"]): string {
  return date.kind === "known" ? date.date : `Uncertain: ${date.original}`;
}

function Source({
  source,
  label = "Original source",
}: {
  source: Model["Provenance"];
  label?: string;
}): ReactElement {
  const isWeb = /^https:\/\//u.test(source.url);
  return (
    <p className="source">
      {source.confidence} · {source.attribution}{" "}
      {isWeb ? <a href={source.url}>{label}</a> : <span>{source.url}</span>}{" "}
      {/^https:\/\//u.test(source.terms_url) && <a href={source.terms_url}>Terms of use</a>}
    </p>
  );
}

function Diagnostics({ items }: { items: Model["Diagnostic"][] }): ReactElement {
  return (
    <div>
      {items.map((item) => (
        <p className="diagnostic" key={`${item.code}-${item.locator}-${item.message}`}>
          <strong>{item.code}</strong>: {item.message}
          <br />
          <code>{item.locator}</code>
        </p>
      ))}
    </div>
  );
}

function Coverage({ coverage }: { coverage: Model["Coverage"] }): ReactElement {
  return (
    <div className="coverage">
      <p>
        {coverage.stored_revisions} stored revisions
        {coverage.available_revisions != null
          ? ` of ${coverage.available_revisions} available`
          : "; source inventory absent"}
        . Earliest known enforcement: {coverage.earliest_known ?? "unconfirmed"}.{" "}
        {coverage.enactment_known
          ? "Enactment text is recorded."
          : "Enactment text is not established."}
      </p>
      <p>Last collected: {coverage.last_collected ?? "unavailable"}.</p>
      <Diagnostics items={coverage.diagnostics} />
    </div>
  );
}

function DiffView({ diff }: { diff: Model["Diff"] }): ReactElement {
  return (
    <section aria-label="Structural changes">
      <p>{diff.changes.length} structural changes.</p>
      {diff.changes.map((change) => (
        <article
          className="change"
          key={`${change.kind}-${change.after?.path ?? change.before?.path}`}
        >
          <h3>{change.kind}</h3>
          <p>
            <code>{change.after?.path ?? change.before?.path}</code>
          </p>
          {change.before && (
            <div className="removed">
              <strong>− Removed text</strong>
              <p>{text(change.before.element)}</p>
            </div>
          )}
          {change.after && (
            <div className="added">
              <strong>+ Added text</strong>
              <p>{text(change.after.element)}</p>
            </div>
          )}
        </article>
      ))}
      <Diagnostics items={diff.diagnostics} />
    </section>
  );
}

function positionKey(position: Model["Position"]): string {
  return `${position.law_id}|${position.version}|${position.path}`;
}

function referenceKey(edge: Model["Reference"]): string {
  return `${positionKey(edge.from)}|${edge.kind}|${edge.text}|${edge.to ? positionKey(edge.to) : "unresolved"}`;
}

function ReferenceView({ edges }: { edges: Model["Reference"][] }): ReactElement {
  return (
    <section aria-label="Reference paths">
      {edges.length === 0 && <p>No recognized references in the selected corpus.</p>}
      {edges.map((edge) => (
        <details key={referenceKey(edge)}>
          <summary>
            {edge.text} · {edge.kind} · {edge.to ? "resolved candidate" : "unresolved"}
          </summary>
          <p>
            From{" "}
            <code>
              {edge.from.law_id} {edge.from.path}
            </code>
          </p>
          <p>
            To <code>{edge.to ? `${edge.to.law_id} ${edge.to.path}` : "Unconfirmed target"}</code>
          </p>
          <p>Confidence: {edge.confidence}.</p>
          <Diagnostics items={edge.diagnostics} />
        </details>
      ))}
    </section>
  );
}

function ImpactView({ impact }: { impact: Model["Impact"] }): ReactElement {
  return (
    <section aria-label="Impact paths">
      <h3>Direct changes</h3>
      {impact.direct.map((position) => (
        <p key={positionKey(position)}>
          <code>
            {position.law_id} {position.path}
          </code>
        </p>
      ))}
      <h3>Review through references</h3>
      {impact.review.map((item) => (
        <details key={positionKey(item.position)}>
          <summary>
            {item.position.law_id} {item.position.path}
          </summary>
          {item.via.map((edge) => (
            <p key={referenceKey(edge)}>
              {edge.text}: <code>{edge.from.path}</code> →{" "}
              <code>{edge.to?.path ?? "unresolved"}</code>
            </p>
          ))}
        </details>
      ))}
      <p>{impact.broken.length} unresolved references in the selected corpus.</p>
      <Diagnostics items={impact.diagnostics} />
    </section>
  );
}

interface LocationState {
  q: string;
  at: string;
  knownAt: string;
  law: string;
  version: string;
  from: string;
  to: string;
  scenario: string;
}
function locationState(): LocationState {
  const params = new URLSearchParams(window.location.search);
  return {
    q: params.get("q") ?? "",
    at: params.get("at") ?? "",
    knownAt: params.get("known_at") ?? "",
    law: params.get("law") ?? "",
    version: params.get("version") ?? "",
    from: params.get("from") ?? "",
    to: params.get("to") ?? "",
    scenario: params.get("scenario") ?? "",
  };
}
function timeOf(state: LocationState): Temporal {
  return {
    ...(state.at ? { at: state.at } : {}),
    ...(state.knownAt ? { known_at: state.knownAt } : {}),
    ...(state.version ? { version: state.version } : {}),
  };
}

function provisions(node: LawElement, path = "/Law"): { path: string; node: LawElement }[] {
  const counts = new Map<string, number>();
  const output = ["Article", "SupplProvision", "AppdxTable"].includes(node.tag)
    ? [{ path, node }]
    : [];
  if (output.length) return output;
  for (const content of node.content ?? []) {
    if (content.kind === "element") {
      const child = content.value;
      const ordinal = (counts.get(child.tag) ?? 0) + 1;
      counts.set(child.tag, ordinal);
      const segment = child.attributes?.Num
        ? `${child.tag}[num=${child.attributes.Num}]`
        : `${child.tag}[index=${ordinal}]`;
      output.push(...provisions(child, `${path}/${segment}`));
    }
  }
  return output;
}

export function App(): ReactElement {
  const [location, setLocation] = useState<LocationState>(locationState);
  const [searchText, setSearchText] = useState(location.q);
  const [at, setAt] = useState(location.at);
  const [knownAt, setKnownAt] = useState(location.knownAt);
  const [hits, setHits] = useState<Model["SearchHit"][]>([]);
  const [searchDiagnostics, setSearchDiagnostics] = useState<Model["Diagnostic"][]>([]);
  const [selection, setSelection] = useState<Model["Selection"] | null>(null);
  const [history, setHistory] = useState<Model["StoredRevision"][]>([]);
  const [edges, setEdges] = useState<Model["Reference"][]>([]);
  const [referenceDiagnostics, setReferenceDiagnostics] = useState<Model["Diagnostic"][]>([]);
  const [diff, setDiff] = useState<Model["Diff"] | null>(null);
  const [impact, setImpact] = useState<Model["Impact"] | null>(null);
  const [parliament, setParliament] = useState<Model["ParliamentaryContext"] | null>(null);
  const [status, setStatus] = useState<Model["Status"] | null>(null);
  const [from, setFrom] = useState(location.from);
  const [to, setTo] = useState(location.to);
  const [scenarioText, setScenarioText] = useState("");
  const [originalText, setOriginalText] = useState("");
  const [replacementText, setReplacementText] = useState("");
  const [targetPath, setTargetPath] = useState("");
  const [scenarioAt, setScenarioAt] = useState("");
  const [simulation, setSimulation] = useState<Model["Simulation"] | null>(null);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);

  function navigate(update: Partial<LocationState>): void {
    const state = { ...location, ...update };
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(state))
      if (value) params.set(key === "knownAt" ? "known_at" : key, value);
    window.history.pushState(null, "", `?${params.toString()}`);
    setLocation(state);
  }

  useEffect(() => {
    const changed = (): void => {
      const state = locationState();
      setLocation(state);
      setSearchText(state.q);
      setAt(state.at);
      setKnownAt(state.knownAt);
      setFrom(state.from);
      setTo(state.to);
    };
    window.addEventListener("popstate", changed);
    return (): void => window.removeEventListener("popstate", changed);
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    setError("");
    setBusy(true);
    setSelection(null);
    setDiff(null);
    setImpact(null);
    setParliament(null);
    const time = timeOf(location);
    async function load(): Promise<void> {
      try {
        const searchTime = { ...time };
        delete searchTime.version;
        const [found, currentStatus] = await Promise.all([
          loadSearch(location.q, searchTime, controller.signal),
          loadStatus(controller.signal),
        ]);
        if (controller.signal.aborted) return;
        setHits(found.hits);
        setSearchDiagnostics(found.diagnostics);
        setStatus(currentStatus);
        if (location.law) {
          const [selected, revisions, references, debate] = await Promise.all([
            loadLaw(location.law, time, controller.signal),
            loadHistory(location.law, time, controller.signal),
            loadRefs(location.law, time, controller.signal),
            loadParliament(location.law, controller.signal),
          ]);
          if (controller.signal.aborted) return;
          setSelection(selected);
          setHistory(revisions);
          setEdges(references.references);
          setReferenceDiagnostics(references.diagnostics);
          setParliament(debate);
          setFrom(location.from || revisions[0]?.version || "");
          setTo(location.to || selected.selected.version);
        }
        if (location.from && location.to) {
          const [difference, affected] = await Promise.all([
            loadDiff(location.from, location.to, controller.signal),
            loadImpact(location.from, location.to, time, controller.signal),
          ]);
          if (controller.signal.aborted) return;
          setDiff(difference);
          setImpact(affected);
        }
        if (location.scenario) {
          const saved = await loadScenario(location.scenario, controller.signal);
          if (!controller.signal.aborted) setScenarioText(JSON.stringify(saved, null, 2));
        }
      } catch (failure) {
        if (!controller.signal.aborted) setError(errorMessage(failure));
      } finally {
        if (!controller.signal.aborted) setBusy(false);
      }
    }
    void load();
    return (): void => controller.abort();
  }, [location]);

  function search(event: FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    navigate({ q: searchText, at, knownAt, law: "", version: "", from: "", to: "" });
  }

  function createScenario(): void {
    if (!selection) return;
    const revision = selection.selected.revision;
    const target = provisions(revision.tree)[0];
    const effectiveDate =
      scenarioAt ||
      location.at ||
      (revision.effective.kind === "known" ? revision.effective.date : "");
    const scenario: Scenario = {
      schema_version: 1,
      name: "Proposed amendment",
      bases: { [revision.law_id]: selection.selected.version },
      at: effectiveDate,
      known_at: location.knownAt || new Date().toISOString(),
      steps: [
        {
          id: "proposed-amendment",
          effective: {
            kind: "known",
            date: effectiveDate,
            original: effectiveDate,
            basis: "Scenario assumption",
          },
          after: [],
          adjustments: [],
          operations: [
            {
              operation: {
                kind: "replace_text",
                law_id: revision.law_id,
                path: targetPath || target?.path || "/Law",
                old: originalText,
                new: replacementText,
              },
              evidence: { ...revision.source, confidence: "inferred", locator: "Scenario editor" },
            },
          ],
        },
      ],
    };
    setScenarioText(JSON.stringify(scenario, null, 2));
    setSimulation(null);
    setMessage("");
  }

  async function runScenario(save: boolean): Promise<void> {
    setError("");
    setMessage("");
    setBusy(true);
    try {
      const value: unknown = JSON.parse(scenarioText);
      const scenario = scenarioSchema.parse(value);
      const result = await simulateScenario(scenario);
      setSimulation(result);
      if (save && result.diagnostics.length === 0) {
        const id = await saveScenario(scenario);
        navigate({ scenario: id });
        setMessage("Scenario saved. The URL identifies the saved scenario in this store.");
      }
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <a className="skip" href="#main">
        Skip to search
      </a>
      <header>
        <h1>law-history</h1>
        <p>Japanese legislation by enforcement date and observation time</p>
      </header>
      <main id="main">
        <section aria-labelledby="search-heading">
          <h2 id="search-heading">Find a law</h2>
          <form onSubmit={search} className="search-form">
            <div>
              <label htmlFor="query">Law title, ID, or provision text</label>
              <input
                id="query"
                value={searchText}
                onChange={(event) => setSearchText(event.target.value)}
              />
            </div>
            <div>
              <label htmlFor="at">Enforcement date</label>
              <input
                id="at"
                type="date"
                value={at}
                onChange={(event) => setAt(event.target.value)}
              />
              <p className="hint">Blank uses the current date.</p>
            </div>
            <div>
              <label htmlFor="known-at">Known at (RFC 3339)</label>
              <input
                id="known-at"
                value={knownAt}
                onChange={(event) => setKnownAt(event.target.value)}
                placeholder="2026-01-01T00:00:00Z"
              />
              <p className="hint">Blank uses all collected information.</p>
            </div>
            <button type="submit">Search</button>
          </form>
        </section>
        <div role="status" aria-live="polite">
          {busy ? "Loading…" : message}
        </div>
        {error && (
          <p role="alert" className="diagnostic">
            {error}
          </p>
        )}
        <div className="workspace">
          <section className="results" aria-labelledby="results-heading">
            <h2 id="results-heading">Search results</h2>
            <p>{hits.length} laws in the selected local corpus.</p>
            <Diagnostics items={searchDiagnostics} />
            {hits.map((hit) => (
              <article key={hit.law_id}>
                <h3>
                  <button
                    type="button"
                    className="text-button"
                    onClick={() =>
                      navigate({ law: hit.law_id, version: hit.version, from: "", to: "" })
                    }
                  >
                    {hit.title}
                  </button>
                </h3>
                <p>
                  <code>{hit.law_id}</code>
                </p>
                <p>Effective: {dateText(hit.effective)}</p>
              </article>
            ))}
            {status && (
              <details>
                <summary>Collection coverage</summary>
                <p>{status.laws.length} laws have stored text.</p>
                <p>
                  {status.catalog
                    ? `${status.catalog.law_ids.length} catalog entries of ${status.catalog.reported_count} reported; collected ${status.catalog.collected_at}.`
                    : "The e-Gov catalog has not been collected."}
                </p>
                <Diagnostics items={status.diagnostics} />
              </details>
            )}
          </section>
          <div className="reader">
            {selection ? (
              <>
                <section aria-labelledby="law-heading">
                  <h2 id="law-heading">{selection.selected.revision.title}</h2>
                  <p>
                    Effective: {dateText(selection.selected.revision.effective)} · Law promulgated:{" "}
                    {dateText(selection.selected.revision.promulgated)}
                  </p>
                  {selection.selected.revision.amendment_promulgated && (
                    <p>
                      Amendment promulgated:{" "}
                      {dateText(selection.selected.revision.amendment_promulgated)}
                    </p>
                  )}
                  <p>
                    Version <code>{selection.selected.version}</code>
                  </p>
                  {selection.selected.revision.repeal && (
                    <p>Repeal: {selection.selected.revision.repeal}</p>
                  )}
                  {selection.selected.revision.termination && (
                    <p>
                      Termination evidence: {selection.selected.revision.termination.status} ·{" "}
                      {dateText(selection.selected.revision.termination.date)}
                    </p>
                  )}
                  <Source source={selection.selected.revision.source} />
                  {selection.selected.revision.metadata_source && (
                    <Source
                      source={selection.selected.revision.metadata_source}
                      label="Revision metadata source"
                    />
                  )}
                  <Coverage coverage={selection.coverage} />
                  <Diagnostics items={selection.diagnostics} />
                  <details>
                    <summary>Selected law text</summary>
                    {provisions(selection.selected.revision.tree).map((item) => (
                      <article key={item.path}>
                        <h3>
                          {item.node.tag} {item.node.attributes?.Num ?? ""}
                        </h3>
                        <p lang="ja">{text(item.node)}</p>
                        <p>
                          <code>{item.path}</code>
                        </p>
                      </article>
                    ))}
                  </details>
                </section>
                <section aria-labelledby="compare-heading">
                  <h2 id="compare-heading">Compare revisions</h2>
                  <form
                    onSubmit={(event) => {
                      event.preventDefault();
                      navigate({ from, to });
                    }}
                    className="compare-form"
                  >
                    <div>
                      <label htmlFor="from">Before revision</label>
                      <select
                        id="from"
                        value={from}
                        onChange={(event) => setFrom(event.target.value)}
                      >
                        {history.map((item) => (
                          <option key={item.version} value={item.version}>
                            {dateText(item.revision.effective)} · {item.revision.revision_id}
                          </option>
                        ))}
                      </select>
                    </div>
                    <div>
                      <label htmlFor="to">After revision</label>
                      <select id="to" value={to} onChange={(event) => setTo(event.target.value)}>
                        {history.map((item) => (
                          <option key={item.version} value={item.version}>
                            {dateText(item.revision.effective)} · {item.revision.revision_id}
                          </option>
                        ))}
                      </select>
                    </div>
                    <button type="submit">Compare</button>
                  </form>
                  {diff && <DiffView diff={diff} />}
                  {impact && <ImpactView impact={impact} />}
                </section>
                <section aria-labelledby="refs-heading">
                  <h2 id="refs-heading">References and delegation</h2>
                  <ReferenceView edges={edges} />
                  <Diagnostics items={referenceDiagnostics} />
                </section>
                <section aria-labelledby="parliament-heading">
                  <h2 id="parliament-heading">Bills and parliamentary debate</h2>
                  {parliament?.bills.length === 0 && (
                    <p>No evidenced bill linkage is stored for the selected law.</p>
                  )}
                  {parliament?.bills.map((bill) => (
                    <article
                      key={`${bill.id.house}-${bill.id.kind}-${bill.id.session}-${bill.id.number}`}
                    >
                      <h3>{bill.title}</h3>
                      <p>
                        {bill.id.house} · {bill.id.kind} · Session {bill.id.session} · Bill{" "}
                        {bill.id.number}
                      </p>
                      <p>Submitters: {bill.submitters.join(", ")}</p>
                      <p>{bill.reason}</p>
                      {bill.speeches.map((link) => (
                        <p key={link.speech_id}>
                          Speech {link.speech_id} · Link confidence: {link.confidence} ·{" "}
                          {link.basis}
                        </p>
                      ))}
                      <Source source={bill.source} />
                    </article>
                  ))}
                  {parliament?.speeches.map((speech) => (
                    <details key={speech.speech_id}>
                      <summary>
                        {speech.speaker} · {speech.date} · {speech.meeting}
                      </summary>
                      <p lang="ja">{speech.text}</p>
                      <a
                        href={/^https:\/\//u.test(speech.url) ? speech.url : "#parliament-heading"}
                      >
                        Original speech
                      </a>
                      <Source source={speech.source} />
                    </details>
                  ))}
                </section>
              </>
            ) : (
              <section>
                <h2>Law text and history</h2>
                <p>Select a search result to inspect its sources and revisions.</p>
              </section>
            )}
            <section aria-labelledby="scenario-heading">
              <h2 id="scenario-heading">Hypothetical amendment</h2>
              {selection && (
                <form
                  onSubmit={(event) => {
                    event.preventDefault();
                    createScenario();
                  }}
                  className="scenario-form"
                >
                  <div>
                    <label htmlFor="scenario-target">Provision to amend</label>
                    <select
                      id="scenario-target"
                      value={
                        targetPath ||
                        provisions(selection.selected.revision.tree)[0]?.path ||
                        "/Law"
                      }
                      onChange={(event) => setTargetPath(event.target.value)}
                    >
                      {provisions(selection.selected.revision.tree).map((item) => (
                        <option key={item.path} value={item.path}>
                          {item.node.tag} {item.node.attributes?.Num ?? ""} ·{" "}
                          {text(item.node).slice(0, 80)}
                        </option>
                      ))}
                      {provisions(selection.selected.revision.tree).length === 0 && (
                        <option value="/Law">Complete law</option>
                      )}
                    </select>
                  </div>
                  <div>
                    <label htmlFor="scenario-date">Amendment enforcement date</label>
                    <input
                      id="scenario-date"
                      type="date"
                      required
                      value={
                        scenarioAt ||
                        location.at ||
                        (selection.selected.revision.effective.kind === "known"
                          ? selection.selected.revision.effective.date
                          : "")
                      }
                      onChange={(event) => setScenarioAt(event.target.value)}
                    />
                  </div>
                  <div>
                    <label htmlFor="original-text">Original text</label>
                    <input
                      id="original-text"
                      required
                      value={originalText}
                      onChange={(event) => setOriginalText(event.target.value)}
                    />
                  </div>
                  <div>
                    <label htmlFor="replacement-text">Replacement text</label>
                    <input
                      id="replacement-text"
                      value={replacementText}
                      onChange={(event) => setReplacementText(event.target.value)}
                    />
                  </div>
                  <button type="submit">Create scenario from selected revision</button>
                </form>
              )}
              <p>Simulation uses pinned base versions. Saving requires the explicit save action.</p>
              <label htmlFor="scenario">Scenario JSON</label>
              <textarea
                id="scenario"
                rows={16}
                value={scenarioText}
                onChange={(event) => {
                  setScenarioText(event.target.value);
                  setSimulation(null);
                }}
                spellCheck={false}
              />
              <div className="actions">
                <button type="button" onClick={() => void runScenario(false)}>
                  Simulate
                </button>
                <button type="button" onClick={() => void runScenario(true)}>
                  Save scenario for sharing
                </button>
              </div>
              {location.scenario && (
                <p>
                  Saved scenario{" "}
                  <a href={`?scenario=${encodeURIComponent(location.scenario)}`}>
                    {location.scenario}
                  </a>
                </p>
              )}
              {simulation && (
                <section aria-label="Simulation result">
                  <h3>Simulation result</h3>
                  <ImpactView impact={simulation.impact} />
                  <p>{simulation.applied_steps.length} applied steps.</p>
                  <Diagnostics items={simulation.diagnostics} />
                  {simulation.diffs.map((difference) => (
                    <DiffView key={difference.from} diff={difference} />
                  ))}
                </section>
              )}
            </section>
          </div>
        </div>
      </main>
      <footer>
        <p>Coverage and inferred references accompany the collected text.</p>
        <a href="https://laws.e-gov.go.jp/">e-Gov Law Search</a>
        {" · "}
        <a href="https://kokkai.ndl.go.jp/">National Diet Library proceedings</a>
        {" · "}
        <a href="/api/openapi.json">OpenAPI</a>
      </footer>
    </>
  );
}
