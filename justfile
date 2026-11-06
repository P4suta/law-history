default:
    @just --list

check:
    cargo xtask check

release-check corpus:
    cargo xtask release-check --corpus {{ corpus }}

schema:
    cargo xtask schema

proofs:
    cargo xtask proofs

fuzz:
    cargo xtask fuzz

mutations *args:
    cargo xtask mutations {{ args }}

audit:
    cargo xtask audit

demo:
    cargo run --package law-history-cli -- --store .law-history sync --import fixtures/demo.json

serve:
    cargo run --package law-history-cli -- --store .law-history serve
