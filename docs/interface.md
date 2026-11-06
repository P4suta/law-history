# Interface contracts

The main heading names `law-history`.
The public website places collection controls before corpus search because a browser begins with an empty corpus.
Corpus search precedes its results and the law reader.
The [W3C heading guidance](https://www.w3.org/WAI/tutorials/page-structure/headings/) supports descriptive headings and a meaningful hierarchy.

Every search field, revision selector, and scenario editor has a visible label associated through `for` and `id`.
The [W3C labeling tutorial](https://www.w3.org/WAI/tutorials/forms/labels/) requires controls to have labels identifying their purpose.
Native form submission, buttons, selectors, and disclosure elements supply keyboard behavior.
Visible focus outlines preserve the active control.

Official catalog search and corpus import have visible labels and native keyboard controls under the same [W3C labeling guidance](https://www.w3.org/WAI/tutorials/forms/labels/).
Collection reports progress through a status region and retains completed revisions when stopped.
The import description states that snapshots replace the browser corpus before users select a file.
Scenario sharing describes the pinned recipe in the URL and the corpus file needed to transfer exact source versions.
Provision and difference views map source ruby annotations to the [HTML `ruby` and `rt` elements](https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-ruby-element).
Provision selectors use base text so annotation text doesn't become part of an amendment target preview.
The [keyboard criterion explanation](https://www.w3.org/WAI/WCAG22/Understanding/keyboard.html) requires operations to support a keyboard interface.

Black and white distinguish surfaces and text.
A diff uses solid and dashed borders with explicit added and removed text labels.
The [W3C explanation of use of color](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html) supports supplying information through another visual means alongside color.
Browser checks inspect the parsed CSS rules, computed hover and focus states, labels, narrow layout, URL restoration, and the main search and simulation workflows.

The URL preserves search conditions, enforcement and observation cutoffs, selected versions, and comparison versions.
Scenario persistence requires an explicit save action.
The saved identifier is local to the selected store.
Coverage, source attribution, inferred reference confidence, and parsing diagnostics remain visible beside the relevant result.
