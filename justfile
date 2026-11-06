default:
    @just --list

check:
    cargo xtask check

[positional-arguments]
release-check corpus:
    cargo xtask release-check --corpus "$@"

schema:
    cargo xtask schema

proofs:
    cargo xtask proofs

fuzz:
    cargo xtask fuzz

[positional-arguments]
mutations *args:
    cargo xtask mutations "$@"

alias mutants := mutations

model:
    cargo xtask browser-model

pages:
    cargo xtask build-browser

audit:
    cargo xtask audit

demo:
    cargo run --package law-history-cli -- --store .law-history sync --import fixtures/demo.json

serve:
    cargo run --package law-history-cli -- --store .law-history serve
