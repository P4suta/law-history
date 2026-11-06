# Assurance scope

The production `law-domain` crate supplies validated Gregorian calendar days, temporal classification, decimal digit recognition and accumulation, amendment precondition decisions, structural classification, observation arbitration, schema-name validation, and unique text replacement.
Kani imports those functions directly.
Kani also imports the production CLI error-channel decision.
The [proof inventory](../verification/obligations.json) identifies the required harnesses.
The verification command requires exactly the 35 named harnesses, the pinned verifier version, and a successful inventory summary.
Missing harnesses, tool errors, and resource stops can't produce a passing receipt.
Every reported cover property must have a witness.
The inventory requires exact witness counts for the termination, provision-context, ruby-context, revision-identity, inline-attribute, native-log, image-digest, and cover-evidence contracts.
Kani checks the production cover-count decision over arbitrary machine-size counts and optional requirements.
Eight cover properties exercise absent evidence, zero counts, partial and excessive counts, and complete counts with matching or mismatching requirements.
The proof gate also requires a deliberate false claim about the production amendment decision to produce its expected assertion counterexample.
The negative control uses a distinct invocation with the `proof-negative-control` feature and must fail in its named harness with the expected assertion message.
An unwinding failure, missing harness, different assertion, or resource stop can't pass the control.

Calendar construction admits years 1 through 9999 and valid month and day components.
The calendar proof checks the exact success or error result over every `u16` year and `u8` month and day.
Separate harnesses check chronological ordering, inclusive enforcement and repeal boundaries, future states, and uncertain dates over arbitrary admitted dates.
The enforcement order harness checks arbitrary revision and distinct-sequence counts.
It covers empty groups, single revisions without sequence evidence, and refusal of incomplete or inconsistent groups with at least two revisions.
The amendment harness checks all eight Boolean combinations of base identity, original text agreement, and conflict state, including failure precedence.
The XML adapter supplies the actual equality results to the precondition function.
The precondition theorem doesn't establish the correctness of XML parsing or Japanese legal interpretation.

The evidence-choice harness checks every Boolean presence and equality combination.
Provided values survive a complementary missing observation, and distinct provided values remain a conflict.
The termination-evidence harness treats absent, uncertain, and known date evidence independently of the termination-status claim.
It requires witnesses for all six combinations of the date state and the Boolean claim.
Owned enum inputs derive Kani's arbitrary generator from the production type instead of maintaining a separate list of admitted variants.
Native selection regressions apply that distinction to imported records, preserved records, and e-Gov responses.
An explicit version remains inspectable with an unknown-date diagnostic.
The source adapter uses a closed repeal-status type and refuses unknown variants before interpreting an enforcement state.
Missing, malformed, and unconverted repeal dates remain uncertain rather than implying an active law.
Ninety source combinations cross five statuses, two amendment types, and nine date representations.
They check exact preservation, default selection, pinned diagnostics, and index reconstruction through the collection entry point.
Supplied legal repeal dates remain evidence when the status is `None`, while processing dates remain uncertain legal dates.

Current-state evidence checks arbitrary valid acquisition and query days and both candidate-match states.
An independent native calendar-tuple oracle crosses nine dates around the Unix epoch, year boundaries, and a leap day with both candidate states.
The production selector uses the decision only for a matching revision whose enforcement date is greatest among the candidates.
The adapter retains the original HTTP acquisition time independently of local cache observation and inventory parsing time.
The inventory binds its current marker to the content address of a body whose revision metadata agrees with the history response.
Missing bodies, wrong receipt subjects, and later content corrections can't reuse that marker for different content.
An acquisition time absent from an imported cache remains absent until an HTTP revalidation supplies evidence.
Native regressions check the Japan Standard Time date boundary, past query dates, knowledge cutoffs, absent or repeated current markers, sequence conflicts, cache replay, and index reconstruction.
The [official API schema](https://laws.e-gov.go.jp/api/2/swagger-ui/lawapi-v2.yaml) defines `CurrentEnforced` as a revision in force at the source snapshot.
That external status is snapshot evidence and doesn't establish a historical amendment order.

Kani checks decimal recognition over every Rust `char` and admits exactly the ASCII and fullwidth decimal codepoints.
The fullwidth mapping follows the [Unicode character chart](https://www.unicode.org/charts/PDF/UFF00.pdf) for U+FF10 through U+FF19.
Kani checks decimal accumulation over every `u32` prefix and `u8` digit.
The proof covers invalid digits and overflow.
Native regressions check every Unicode scalar value and independently calculate accumulation in a wider integer.
Reference regressions and a dedicated fuzz target preserve complete article and item branch chains.
Invalid optional numbers remain unresolved instead of falling back to a parent provision.
The grammar and its interpretation remain outside the arithmetic proofs.

Separate harnesses check every Boolean structural-classification and CLI error-channel combination.
Observation arbitration checks each ordering and identity combination.
It retains newer observations and refuses conflicting ties.
Selection regressions verify that a later correction resolves an older conflict while an earlier knowledge cutoff preserves the conflict.
Selection, coverage, and chronological attribution share arbitration by observation time.
Conflicting observations at a revision's greatest observation timestamp remain diagnostics and can't establish an enactment or earliest confirmed date.
Chronological attribution rejects unknown enforcement dates and unordered same-day groups and stops at a pinned revision's sequence.
The schema-name harness checks arbitrary byte sequences of length zero through four with an unwind bound of eight.
Native tests exhaust every one-byte and two-byte schema name and exercise longer names through XML regressions and fuzzing.
The number-address harness uses the same bounds and checks that a numbered address is nonempty, differs from the zero sentinel, and contains no path delimiters.
Native tests check every one-byte and two-byte number value.

Text replacement requires exactly one match, including overlapping matches, and preserves the surrounding UTF-8 text.
Seven separate replacement harnesses cover unique, absent, repeated, overlapping, empty, prefix, and suffix cases with inputs of at most nine bytes and an unwind bound of 16.
The matching-decision harness checks error precedence and span arithmetic over arbitrary machine-size lengths, optional first positions, and repetition flags.
The replacement-planning harness checks span order, bounds, exact retained lengths, and the inclusive output limit over arbitrary machine-size inputs.
The allocation-size harness independently checks overflow and the 32 MiB output limit.
The production replacement function uses the checked plan to construct the resulting text.
Rust string search supplies first-match positions and repetition flags.
An independent native oracle uses the alphabet `a`, `甲`, and `乙`.
It enumerates strings of up to five symbols and needles of up to three symbols, including overlapping matches.
It checks replacement bytes for three insertions.
Native tests exercise the actual allocation boundary with a 32 MiB result and reject output exceeding 32 MiB.
The XML adapter calls the verified replacement function and rejects replacements that cross element boundaries.
Adapter regressions check input preservation and atomic failure.
The string-library adapter and arbitrary legal text remain outside the general arithmetic proofs.
A direct symbolic string-search attempt exhausted the bounded runner's 2 GiB memory allowance.
Changing the solver didn't provide a completed result.
Those attempts supply no successful proof evidence.
Kani checks the supported decision core directly.
The native oracle, concrete UTF-8 harnesses, and fixed-seed fuzz target assess the adapter.
Expanding native string support in the verifier or changing the search implementation requires reassessing the adapter proof.

Mutation testing uses the pinned cargo-mutants 27.1.0 over the production `law-domain`, `law-history`, and CLI packages.
The mutation configuration excludes the Kani-only harness module because its assertions aren't part of the native runtime under mutation.
Runtime source and native regression tests remain in scope.
The domain scope selects mutations in `law-domain` while running workspace tests, including consumers of source files shared with the CLI and verification tooling.
Direct domain regressions also check all 36 provision-context transitions and the current-state decision independently of adapter tests and Kani.
Each mutation container owns a fresh anonymous Docker volume for compiler output with the local volume driver.
Shards don't share writable build directories, and every shard runs its unmutated baseline.
Mutation subprocesses use a private 512 MiB tmpfs scratch mount within the unchanged 2 GiB memory limit and have no external network access.
Local HTTP fixtures use loopback, and the regular native gate still checks the host's real filesystem.
The [local-build-storage decision](adr/use-linux-local-mutation-build-storage.md) removes host sharing from compiler output and requires owned volume cleanup.
Each run copies the exact hashed source and test inventory into an immutable container mount before discovery and execution.
Proof and fuzz campaigns use the same snapshot mechanism and refuse changed source identities.
The runner retains completed native baselines, candidate identities, compiler diagnostics, test failures, tool versions, and source hashes with the reports.
Build rejection and detected behavioral changes have distinct counts.
Build rejection requires a native Cargo JSON compiler diagnostic with error severity, including diagnostics without an error code, as defined by the [Cargo message format](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages).
[Rust error E0786](https://doc.rust-lang.org/error_codes/E0786.html) indicates invalid compiler metadata and refuses evidence even when another ordinary error appears before or after it.
Native diagnostic controls cross those positions and 27 combinations of message kind, severity, and code so an earlier valid error can't hide a later infrastructure error.
Infrastructure text checks use native diagnostic prefixes instead of scanning quoted source and assertion messages for isolated words.
Typed compiler diagnostics expose message text and child diagnostics while excluding rendered source spans and error explanations.
Controls distinguish actual process and compiler failures from quoted signal names in test failures and Cargo JSON source spans.
The gate refuses structured resource failures in primary messages and nested notes.
The serialized diagnostic field inventory requires one distinguishing admission probe per consumed field, so adding a field without a probe fails the native gate.
Behavioral detection requires a completed native test failure.
Missing candidates, duplicates, survivors, timeouts, resource stops, inconsistent phases, and changed source identities refuse a complete result.
The full suite requires the union of every shard to equal the complete candidate inventory.
Round-robin partitioning keeps each shard nonempty when the shard count doesn't exceed the candidate count.
No survivor exemptions or score threshold replace the required inventory.
Targeted runs guide development, and the full Rust mutation suite gates a release.
The tooling scope mutates the production native-log size predicate, which Kani imports without translation.
The release command requires the native-diagnostic and selection campaign, native-log predicate campaign, image-reference campaign, and runtime campaign.
Targeted campaigns can select native mutant names through `--matching`, which uses the verifier's [name filter](https://mutants.rs/filter_mutants.html).
Inventory discovery, execution, and retained bounds use the same filter.
The release runtime campaign uses the complete unfiltered inventory.
Completed fuzz and mutation suites emit JSON containing the request bindings, verifier identity, resource bounds, and completed native counts after validating the native results and source inventory.
Proof completion reports the verified harness count and rejected counterexample count after validating both native invocations.
Shared proof inputs use distinct test module names so completed test identities remain unique across workspace executables.
TypeScript remains covered by strict type checking, runtime response validation, generated-contract freshness, and browser regressions rather than a TypeScript mutation score.

The image selector validates both full Docker image IDs and named manifest references through one production boundary.
[Docker's image documentation](https://docs.docker.com/reference/cli/docker/image/pull/) describes image IDs as content addresses.
The boundary requires exactly 64 lowercase hexadecimal digits, following the [OCI digest implementation](https://github.com/opencontainers/go-digest/blob/master/algorithm.go), and refuses mutable tags, empty repository names, extra digest separators, and malformed digests.
Native regressions pass each accepted reference back to the selector, including the image ID representation returned by the default build path.
The earlier separate validators accepted a built ID locally but refused that ID when a mutation worker received it.
Campaigns with an explicit named digest hadn't exercised the default-to-worker transition.
The default campaign now exercises that transition, and build diagnostics go to standard error so completion JSON occupies standard output.
Kani checks the production digest predicate over arbitrary byte sequences of zero through 65 bytes with an unwind bound of 67 and requires valid, invalid-digit, and invalid-length witnesses.
Native tests check all 256 byte values at every digest position and both length boundaries.
The object-codec fuzz target also compares image references with an independent delimiter and character-alphabet oracle.
Docker remains responsible for repository syntax, image availability, and image execution.

The secret scan includes a planted dummy token as a negative control and scans owned source paths with the pinned Gitleaks version.
Dependency auditing uses the RustSec revision in the advisory record.
The npm audit queries the live advisory service.
Those results assess the identified dependencies and advisory inputs rather than proving the absence of every vulnerability.

Rust regressions cover pre-1970 dates, Gregorian leap boundaries, enforcement and observation cutoffs, same-day ambiguity, corrections, staged operations, source mismatch, multi-law failure, reference cycles, and index recovery.
The appendix fixture adapts repeated `Num="0"` elements from an [official e-Gov revision](https://laws.e-gov.go.jp/api/2/law_data/212AC0000000048_20250601_504AC0000000068?law_full_text_format=xml) and adds a synthetic empty-number case.
Zero and empty numbers keep their original attributes and use ordinal addresses within the version.
The numbered-form fixture adapts repeated `AppdxStyle Num="1"` elements from an [official ordinance revision](https://laws.e-gov.go.jp/api/2/law_data/306M10000010022_20230401_505M60000010006?law_full_text_format=xml) and repeated `AppdxFormat Num="15"` elements from an [official form revision](https://laws.e-gov.go.jp/api/2/law_data/322M40000040093_20221104_504M60000040051?law_full_text_format=xml).
The fixture retains those element names and numbers and substitutes synthetic titles.
Repeated style and format numbers use sibling positions, while unique numbered elements keep their numbered paths.
The original `Num` attributes remain unchanged.
The appendix-item fixture adapts repeated `Item Num="1"` siblings from an [official appendix](https://laws.e-gov.go.jp/api/2/law_data/324M50000080026_20250123_507M60000080001?law_full_text_format=xml).
Repeated numbers and numbers containing path delimiters use sibling positions for every element type.
The parser preserves the original attributes instead of rejecting or rewriting the source.
An independent oracle checks every sibling sequence of up to three elements over three element names and eight number states.
It checks the shared child-path builder, flattened elements, and borrowed index.
Move operations resolve source, destination, and insertion positions in one traversal of the original tree.
Regressions check every source and insertion pair in a three-sibling ordinal sequence and a destination whose position shifts after removal.
Browser tests use the real HTTP service and a synthetic corpus.
They exercise search, historical selection, comparison, references, scenario simulation and explicit persistence, URL restoration, keyboard operation, and narrow layout.
Generated TypeScript must match the OpenAPI emitted by the current Rust code.
Response schemas validate data before the app uses it.
Native HTTP tests independently exercise the router, error status and JSON preservation, bounded diagnostics, explicit scenario persistence, source preservation, and a real listening server.
Source-boundary tests use local HTTP responses and preserved catalog pages without calling the public services.
They check response limits, request budgets, cache dates, catalog pagination, source metadata, revision validation, and bill linkage.
The source contract derives the consumed property sets from the production source types and requires one distinguishing semantic probe per field.
Each probe changes one source field and checks its observable projection.
Receipt invalidation and current revision selection have distinct probes.
A typed field without a probe fails the regular test gate.
This check covers consumed fields and doesn't infer the meaning of unmodeled upstream fields retained in the raw response.
The same probe inventory also changes each consumed field in the history response alone.
The collector refuses contradictory provided metadata before publishing a revision or receipt.
Compatible nonempty observations can fill missing or blank fields in the other response.
Each merged revision records both source responses through `source` and `metadata_source`.
Saving, reading, and rebuilding verify both content addresses.
The [complementary-publication fixture](../fixtures/egov-complementary-publication.json) preserves an official history date absent from the corresponding body metadata.
The native regression verifies the date, both original byte sequences, reconstruction of the index, and refusal after either source is unavailable or corrupt.
History snapshots validate all subject identifiers before exposing an expectation bound to its preserved source bytes and revision position.
The `updated` stamp belongs to an individual response and remains a distinct typed field outside the comparable revision metadata.
A changed stamp invalidates its receipt without requiring an equal stamp in the body response.
Preserved official response fixtures verify unequal update stamps for the same revision and bind the original response bytes by SHA-256.
The [scheduled-date fixture](../fixtures/egov-scheduled-enforcement.json) also checks a history response that repeats its scheduled date in the enforcement-date field and a body response that leaves the latter absent.
Both representations remain uncertain enforcement evidence.
An enforcement value equal to a supplied scheduled placeholder contributes no confirmed enforcement date.
Kani checks all Boolean date-role classifications, and an independent union-of-claims oracle checks 625 pairs of actual and scheduled representations.
The optional-field generator discovers nullable fields through the production deserializer and checks every partition of provided values between the two responses.
The generator checks null and blank representations of absent values.
The disagreement cases cover both an initial collection and a resumed collection with an existing receipt.
Receipt invalidation includes law metadata and revision metadata rather than only the latter.
Local HTTP regressions check one revalidation of stale metadata, refusal of a persistent disagreement, exhausted budgets, and the absence of retries for identity and XML failures.
Receipt reuse checks the preserved law and revision identities.
The collection adapter reads its clock after response preservation and at each completed parsing stage.
Local HTTP tests advance the injected clock during a response and verify the resulting cache observation cutoff.
A deterministic advancing clock checks that later revision parsing never enters an earlier batch-start knowledge cutoff.
Cache tests refuse future timestamps and cross-day reuse within one collector.
The adapter trusts the system clock and Chrono's UTC timestamp operations.
Those empirical clock readings aren't certified by the Gregorian calendar proofs.
Law promulgation and amendment promulgation remain separate fields.
Unknown-date selection uses the amendment publication date when the source supplies it, so a later published amendment doesn't make earlier selections uncertain.
Source regressions check both sides of the amendment publication date.
Cache import verifies content addresses, preserves the previous recorded timestamp as archive provenance, and records a new local observation time without importing old history observations.
Native regressions check same-day import, stale and future refusal, existing-cache preservation, corruption refusal, CLI replay, and index reconstruction.

Saving a scenario pins the reference versions and diagnostics for every selected law outside its amendment bases.
The pinned set also fixes which laws were absent from the reference context.
Replay validates each pinned version's law identity and knowledge cutoff.
Native and HTTP regressions verify persistence, refusal of invalid identities, and stable results after late backdated corrections, added laws, and index reconstruction.
These adapter regressions don't prove reference interpretation or source completeness.

Reference and amendment locators share numeral conversion and main-provision ancestry checks.
Quoted articles and unapplied replacement provisions can't replace an actual main-provision target.
The ancestry adapter calls the domain crate's typed provision-context transition.
Kani checks every admitted context and role, exact allowed parent relations, and the absorbing excluded state.
This one-step invariant prevents reentry after an excluded ancestor under arbitrarily long role sequences.
Native tests independently compare every ancestor sequence of length zero through three over 16 structural and excluded element names with an anchored path grammar.
Native regressions cover structural ancestors, absent and repeated targets, fullwidth and branched numbers, malformed optional locators, and ordinal insertion beside repeated numbers.
The reference fuzz generator also places matching numbers inside quotes and unapplied amendments.
Every independent content, repeal, or enforcement change receives a scenario identity and inferred provenance.
The production decision and its source-bound proof check all eight change combinations.
Scenario regressions exercise those combinations through actual amendment operations while preserving the entire unchanged enforcement value.
The scenario revision's amendment promulgation date remains absent until evidence supplies that date.
Browser regressions compare the API and editor's refusal of unknown scenario, step, and operation properties.
Browser response probes also read the declared revision-kind, confidence, and termination-status variants from OpenAPI and require the selection decoder to accept every variant.
Each probe must reach the intercepted boundary, and termination evidence must remain visible with its uncertain date.
Metadata provenance has a distinct source link instead of an indistinguishable second original-source label.
The editor uses [Zod's strict object validation](https://github.com/colinhacks/zod/blob/v4.6.7/packages/docs/content/api.mdx) to refuse those properties instead of stripping them.
The impact oracle compares every directed three-node graph and every nonempty root set with an independent transitive closure.
Each discovered review position enters the worklist after removal from the set of remaining positions.
An edge can't enqueue an already removed review position.
Storage tests exercise actual raw and compressed objects, allocation limits, compaction statistics, index recovery, and refusal of inconsistent representations.
Concurrent writers must preserve one verified object per source.
Publication accepts an existing destination and propagates other filesystem errors without changing their causes.
Every successful publication checks the restored bytes before returning its content address.
Large-object regressions compare every byte while keeping failure diagnostics bounded.
Native tests check byte equality guards directly.
Those tests don't establish cryptographic collision resistance.

The parser campaign has deterministic valid inputs and malformed controls.
Five libFuzzer targets exercise the production XML, legal-date, amending-document, object-codec, and reference-resolution boundaries with a fixed seed, 10000 runs per target, and bounded input size.
The XML target also checks that indexed positions preserve every original element without collisions.
The regular regression gate replays the preserved seeds with known acceptance and rejection outcomes.
The container pins Rust, Kani, cargo-fuzz, and the nightly toolchain.
Fuzz builds use release optimization, debug assertions, overflow checks, and AddressSanitizer with 16 code-generation units.
Targets run sequentially and share a temporary build cache while keeping their generated corpora separate.
Native parsing and external-source integration remain separate from model checking.
Every proof and fuzz run retains native output and process results, including failed runs.

Mutation operators change existing Rust expressions.
They don't supply an absent source-field interpretation or correct an oracle that shares the implementation's assumption.
The source-field probes, independent path grammar, and official structural fixtures check those different boundaries.

Ruby projection preserves original source bytes and complete element trees while excluding reading annotations from title comparison, reference tokens, and base-text search.
Each of the production context transition's nine input combinations has its own reachability check.
An independent ancestor-path oracle checks 4681 bounded element sequences without calling that transition.
The pinned official XML schema supplies every supported title-child relation to distinguishing projection probes.
Preserved official history and body responses check that plain and annotated titles agree while changed base characters remain a conflict.
Generated title representations, source reconstruction, reference counterexamples, body search, and both XML and reference fuzzing exercise the adapters.
Inline attributes require the correct element, attribute name, and value independently.
Kani checks all eight eligibility combinations in the production decision.
The native adapter checks 210 combinations of schema positions, attribute names, values, and XML whitespace padding.
The pinned schema supplies the attribute declarations and permitted values.
The regular gate replays an annotated title seed with a line-style attribute.

The native mutation-log reader admits at most 8 MiB and reads at most one extra byte before refusing excess output.
It checks the byte limit before decoding or scanning the log.
Kani checks the production admission decision over every machine-size length.
Native filesystem cases include the exact limit and one byte on either side.
The filesystem and Rust reader implementation remain external trust assumptions.

## Counterexamples and verification gaps

Collecting preserved official responses exposed assumptions absent from the synthetic fixtures.
One history response and its body response used distinct update stamps for otherwise compatible revision metadata.
Another pair supplied a scheduled placeholder in one response and no actual date in the other.
A third pair supplied an amendment publication date only in the history response.
Those observations exposed both missing interpretation and an overly strict equality rule.
The original fixtures changed both responses together, so they never exercised the disagreement boundary.
Deserializing or hashing a field satisfied the compiler without establishing its effect on selection or provenance.

The source-field contract now discovers consumed fields from production schemas and requires a distinguishing semantic probe for each field.
A temporary unclassified field made the regular gate fail, even when nested schema composition described that field.
Independent date-claim sets and nullable-field partitions check compatibility without reusing the production merge decision as their oracle.
Preserved official bytes check that synthetic compatibility assumptions describe the observed responses.
The typed expectation binds the comparison to its original history source and prevents callers from constructing an unsupported expectation.
The boundary still trusts the upstream meaning of supplied fields and can't establish missing historical material.

Browser review exposed a separate verification gap in the test helper.
The helper accepted a search-result heading before the selected law had loaded.
It now requires the selected law's level-two heading.
Enum probes also require the response mutation to reach the intercepted boundary before checking decoding and visible termination evidence.
Generated types alone check assignment compatibility and don't establish that a handwritten runtime decoder accepts every producer variant.

A cache-import mutation exposed an assertion that checked only total counts.
Rejecting one valid boundary observation and importing one future observation preserved the asserted total count.
The regression now checks the exact imported key set and isolates each observation-time boundary in a separate store.
The cases include the previous UTC day, the start of the collection day, one nanosecond before collection, the collection instant, one nanosecond after collection, and the following UTC day.
An unchanged total count can't conceal a wrong member in those checks.

Crossing termination status with date evidence exposed a different shared assumption.
The source adapter treated `None` as sufficient to discard a supplied repeal date.
Its Boolean proof model represented only a termination claim and a known-date flag, so it couldn't distinguish an absent date from a supplied uncertain date.
The proof correctly established the admitted statement while leaving the statement inadequate for the source boundary.
Single-field probes didn't cross the status with each date representation.
The production date-evidence enum, six input witnesses, and independent ninety-case collection matrix close those specific gaps.

The original proof gate accepted a successful assertion summary even when a cover property had no witness.
Its regression now refuses partial, malformed, duplicate, missing, and inconsistent cover evidence.
The gate's production count decision has its own source-bound proof and native boundary cases.
An intentionally restricted proof input must leave three termination witnesses unreachable and make the gate refuse a successful receipt.
A temporary unclassified date-evidence variant produced an assertion counterexample in the termination harness.
The derived input generator included the added state.
These checks assess declared domains and obligations and don't establish the completeness of the legal model.

An official history title lacked the ruby markup present in the corresponding body metadata.
Literal title equality rejected the compatible responses, and concatenated annotation text broke reference tokens and base-text searches.
The original string-field probes changed plain strings without changing their representation.
Schema-derived child probes and independent annotation-path oracles now exercise that representation boundary.
The original bytes remain available for direct source comparisons.

A scenario mutation survived because the existing text-change examples also changed enforcement.
The enforcement difference concealed the broken content and repeal conditions.
An independent eight-case change matrix detects the exact surviving operator before Kani checks the production decision.

A collection mutation survived in the receipt-reuse branch because selection tests didn't combine reused receipts with revisions sharing an enforcement day.
The regression crosses initial collection and reuse with a current marker on either candidate, no current marker, and duplicate current markers.
It checks exact content addresses and both current and historical selection outcomes.
Saved and reused revisions now share one current-version binding operation.

An inline-attribute mutation survived examples that paired an unknown attribute name with an invalid style value.
The invalid value concealed a broken name or element condition.
The schema-derived attribute matrix changes each condition independently, and the production eligibility decision has a separate eight-state proof.

Large failure logs exposed a separate assurance gap.
Error assertions formatted 64 MiB byte vectors and 32 MiB strings when mutated readers or replacement bounds returned successful payloads.
The native-log gate scanned whole files without a diagnostic byte budget.
The error assertions now check the error branch or returned length without formatting the success payload.
The log gate refuses excess output through a bounded read, and a deliberate oversize-file control first reproduced the old gate's acceptance.
Response and replacement bounds with injected faults still produce test failures with short diagnostics.

A domain-only mutation campaign exposed a test-selection gap that workspace campaigns concealed.
Adapter tests detected broken provision-context and current-state decisions, but the domain package lacked direct native cases for those decisions.
The domain selector also omitted workspace consumers of borrowed production source.
Independent decision matrices now detect those mutations locally, and the selector checks workspace consumers without expanding the selected production package.

An equivalent arm-deletion mutation exposed duplicate leaf rejection beside a wildcard with the same result.
The duplicate arm adds no behavior and has no distinguishing input.
The implementation removes that arm, and the regular Clippy gate requires `match_same_arms` across the workspace and fuzz targets.
A deliberate check of the original branch reproduced the static diagnostic.
The mutation gate keeps requiring zero surviving candidates.

Generated HTTP response keys use named literal constants through the generator's documented Node API.
The transformation targets operation response maps, while response-like fields in domain data keep their original representation.
The actual OpenAPI response inventory independently checks every generated operation and status.
The pinned compiler checks every exported API type and directly compares response-key types for each operation and endpoint.
The compiler's semantics remain a trust assumption for the finite generated contract.
These checks don't prove arbitrary syntax-tree transformation semantics.
The regular TypeScript gate also checks the generator and its SDK declarations without skipping library checks.

The compiler controls exposed an inadequate equality condition.
Structural type equality and mapped object comparisons accepted a numeric response key changed to a quoted string key.
Direct `keyof` comparisons rejected that transformation.
Separate controls require rejection of changed status values, payload types, optionality, and readonly fields.
A fixture distinguishes operation responses from a domain field with the same name.
The original field-name-only traversal rejected that valid domain field.
Inline operations, default and range responses, every standard status, empty inventories, and unsupported status codes have separate controls.

The release reconciliation checks the preserved catalog, available revision inventories, source integrity, and recorded official amendment comparisons.
It rejects an absent corpus or absent comparison evidence.
Collecting every available e-Gov revision doesn't establish missing historical text.
An enactment claim requires evidence identifying the enactment text.

The parser accepts specific amending sentence forms.
Unknown syntax, relative references without adequate context, and unidentified delegated instruments produce diagnostics.
Bill linkages require an imported source record rather than a speech keyword match.
Automated reference candidates and date metadata don't certify legal conclusions.
Expanding the accepted grammar or adding a calendar conversion requires source-derived fixtures, adapter checks, and an updated assessment of the affected invariant.
