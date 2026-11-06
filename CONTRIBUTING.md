# Contributing

## Development setup

Install [mise](https://mise.jdx.dev/getting-started.html), Git, and a C/C++ build toolchain.
Run the installation commands in the [README](README.md) from the repository root.
The project pins Rust, Node.js, and verification tools in `mise.toml` and keeps package lockfiles in Git.
Linux browser setup may require administrator access to install Chromium's system dependencies.

```sh
mise x -- actionlint
mise x -- zizmor --offline --strict-collection --persona pedantic .github/workflows
mise x -- just check
mise x -- just audit
```

The project gate runs secret detection, Rust formatting and Clippy, regressions, generated API checks, TypeScript, and browser workflows.
Continuous integration also runs the workflow analysis commands.
Workflow analysis uses [actionlint](https://github.com/rhysd/actionlint/blob/main/docs/usage.md) and [zizmor's offline mode](https://docs.zizmor.sh/usage/#operating-modes).
Offline analysis doesn't perform audits that require GitHub network access.
The dependency audit uses the pinned RustSec database and a live npm advisory query.

## Report a defect

Use the repository's bug report or source-correction form with a minimal reproducer.
Include the Git commit, command or API request, expected behavior, actual diagnostics, and relevant source URLs or revision addresses.
Use a separate store for the reproducer and keep credentials and private data out of public reports.
Report vulnerabilities through the [private security reporting route](.github/SECURITY.md).

## Change behavior

Keep each pull request focused on one observable problem.
For a defect, first reproduce the failure with the unchanged implementation.
Explain why the existing tests or proof model admitted the defect, then repair the missing invariant or input partition.
Use an independent oracle where sharing the implementation's assumption would conceal the same error.
Keep source-location diagnostics, uncertainty, and provenance distinctions in regression cases.

Run the affected proof, fuzz, and mutation commands described in the [assurance scope](docs/assurance.md).
Record the actual commands, admitted domains, bounds, outcomes, and unresolved evidence in the pull request.
A timeout, resource stop, skipped case, or missing source remains incomplete evidence.
The complete release gate remains separate from the fast pull request workflow.

## Repository documents

Update `repository.toml` when changing the README's summary, installation, or usage text.
Maintainers regenerate the README and code license files from that metadata.
Update [project status](docs/status.md) when source coverage or supported workflows change.
Preserve fixture attribution in [data sources](DATA_SOURCES.md).
Issue forms follow [GitHub's form syntax](https://docs.github.com/en/communities/using-templates-to-encourage-useful-issues-and-pull-requests/syntax-for-issue-forms).

Use English and one sentence per line in documents, comments, and commit messages.
Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) for commit and pull request titles.
Project-authored contributions use `MIT OR Apache-2.0` unless separate terms are explicitly identified.
