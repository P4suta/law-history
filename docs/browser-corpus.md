# Use the browser corpus

Open [law-history on GitHub Pages](https://p4suta.github.io/law-history/).
The website runs the shared Rust history, comparison, reference, and amendment services in WebAssembly.
GitHub Pages serves static assets.
The website fetches selected official histories directly from e-Gov.

Choose **Collect the Constitution of Japan**, or enter a title under **Official law title** and select a result.
Collection preserves the available revision inventory, original responses, structural versions, and observation timestamps in the browser.
Collection saves each completed revision before proceeding.
**Stop collection** retains completed revisions, and another collection attempt can complete the history.
Missing revisions remain visible in collection coverage.
**Load synthetic sample** provides two demonstration revisions without a source request to e-Gov.
The sample isn't legislation.

The search form queries the collected corpus by title, identifier, or provision text.
An empty enforcement date uses the current day in Japan.
An empty knowledge cutoff uses all collected observations.
The URL retains the selected law, temporal conditions, and compared version hashes.
References cover the laws collected in the browser, so collecting another law can change the available reference context.

Simulation uses pinned base hashes and enforcement assumptions.
**Save scenario for sharing** records the scenario and puts its pinned recipe in the URL fragment.
Copy the URL and use **Export corpus** to transfer its exact source versions and saved records to another browser.
The recipe remains readable without the corpus, but simulation refuses missing base versions.
Importing a corpus doesn't change the recipe's base hashes.
Snapshot imports replace the browser corpus.
Fixture imports add revisions.
Export existing work before replacing a corpus.

IndexedDB stores the corpus under the website origin.
Browser storage quotas and eviction can remove stored data, so an exported corpus is the durable transfer copy.
Snapshot restoration checks schema versions, SHA-256 addresses, revision sources, observation references, and batch records before replacing the live state.
A failed import preserves the existing corpus.
A corpus must fit in a 256 MiB export file.
Collection checks the resulting snapshot size before committing an update.
An oversize result leaves the corpus unchanged.
Use the CLI for larger collections.
An explicit valid import can replace a damaged saved corpus.

Build the static site and start its local server:

```sh
mise x -- just pages
mise x -- cargo xtask pages-server
```

Open [the local preview](http://127.0.0.1:37652/law-history/) to inspect the publication bundle.
The [usage guide](usage.md) describes the command-line tool and native server.
The Pages build also generates the shared browser bindings needed by the native Web app.

The [publication workflow](../.github/workflows/pages.yml) builds the protected default branch and deploys the static bundle.
Every change exercises the generated WebAssembly in a browser.
A request to an API route receives status 404 from the static test server.
The browser tests cover history, differences, references, explicit sharing, persistence, invalid imports, damaged saved state, and keyboard use.
The [assurance scope](assurance.md) describes the limits of proofs and source interpretation.
