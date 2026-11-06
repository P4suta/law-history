# Documentation for `law-history`

The [project status](status.md) describes the supported workflows and incomplete corpus coverage.
The [source attribution](../DATA_SOURCES.md) distinguishes project code from upstream data and its terms.
The [contribution guide](../CONTRIBUTING.md) covers reproducible development and defect reports.
The [security reporting guide](../.github/SECURITY.md) provides a private reporting route.
The [usage guide](usage.md) covers collection, temporal selection, amendment scenarios, parliamentary evidence, Git export, and verification commands.
The [persistence decision](adr/preserve-law-revisions-outside-the-search-index.md) describes the source and index boundary.
The [Git data import decision](adr/import-generated-law-data-with-git-fast-import.md) defines the generated file and identity boundary.
The [object compression decision](adr/compress-canonical-objects-and-index-temporal-metadata.md) defines lossless storage and the metadata index.
The [interface contracts](interface.md) record the form, keyboard, and diff display decisions.
The [assurance scope](assurance.md) distinguishes native proofs, regression coverage, and source interpretation.
The [metadata merge decision](adr/merge-compatible-metadata-and-preserve-both-sources.md) defines compatible source claims and preservation of both response sources.
The [mutation storage decision](adr/isolate-mutation-scratch-storage-and-worker-build-caches.md) defines temporary scratch storage, worker build caches, and the real-filesystem verification boundary.
The [proof-domain decision](adr/require-witnesses-for-proof-input-domains.md) requires native witnesses for declared symbolic input domains.
The [ruby projection decision](adr/project-ruby-base-text-without-changing-source-structure.md) preserves source annotations while comparing base text.
The [generated response-key decision](adr/generate-named-http-status-keys-with-checked-types.md) names status constants and checks the resulting API types.
The [Linux build-storage decision](adr/use-linux-local-mutation-build-storage.md) records the current mutation build isolation.
