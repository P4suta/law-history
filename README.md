# Law-history

Preserve and compare Japanese legislation across enforcement and observation time.

## Installation

```sh
mise trust
mise install
mise x -- npm ci --prefix web --ignore-scripts
mise x -- npm exec --prefix web -- playwright install --with-deps chromium
```

## Usage

```sh
mise x -- cargo run --package law-history-cli -- --store .law-history-demo sync --import fixtures/demo.json
mise x -- npm run --prefix web build
mise x -- cargo run --package law-history-cli -- --store .law-history-demo serve
```

## Documentation

[Read the documentation](docs/README.md).

## License

Licensed under the [Apache License, Version 2.0](LICENSE-APACHE) or the [Massachusetts Institute of Technology (MIT) license](LICENSE-MIT), at your option.

Contributions use both licenses unless you state other terms.
