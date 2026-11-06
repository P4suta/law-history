# Data sources and attribution

The project code and project-authored documentation use `MIT OR Apache-2.0`.
Upstream records keep their source terms and aren't relicensed by the code licenses.
The source repository contains regression fixtures rather than a complete law or parliamentary corpus.

## Official law fixtures

The `fixtures/egov-*` responses and [Japanese law XML Schema](https://laws.e-gov.go.jp/file/XMLSchemaForJapaneseLaw_v3.xsd) originate from e-Gov Law Search, Digital Agency, Government of Japan.
The response URLs and uncompressed SHA-256 values appear in [response-clock provenance](fixtures/egov-health-response-clocks.json), [publication provenance](fixtures/egov-complementary-publication.json), [scheduled-enforcement provenance](fixtures/egov-scheduled-enforcement.json), and [ruby-title provenance](fixtures/egov-ruby-title.json).
Files ending in `.json.zst` contain compressed response bytes.

The law-history maintainers prepared metadata projections and reduced XML examples for regression tests.
Derived cases vary source fields, XML context, and annotations to test acceptance and refusal.
Those cases aren't presented as unmodified official records.
The [e-Gov terms](https://www.e-gov.go.jp/terms) require source attribution and identification of editing or processing where applicable.
They apply [Public Data License 1.0](https://www.digital.go.jp/resources/open_data/public_data_license_v1.0) unless separate terms govern the material.
Preserve those notices and the original source links when redistributing source fixtures or derived data.

## Parliamentary records

The [National Diet Library API](https://kokkai.ndl.go.jp/api.html) supplies meeting and speech records under its stated conditions and the [National Diet Library content terms](https://www.ndl.go.jp/sitepolicy/terms).
The API documentation distinguishes library-owned content from speeches whose rights belong to individual speakers.
The code licenses grant no extra rights to those speeches.
Preserve speaker attribution, source URLs, and applicable terms when collecting or exporting parliamentary records.
The collector uses sequential requests with a delay between responses, following the API's access guidance.

## Local stores and synthetic examples

Git ignore rules exclude the default collection store, demo store, build outputs, and local verification reports.
Git export creates a separate repository containing selected preserved data and its provenance.
Review the applicable source terms before publishing that separate data repository.

[The demo](fixtures/demo.json) and fixtures without e-Gov provenance are project-authored examples.
Synthetic examples and reduced parser inputs don't establish that an official amendment or historical revision exists.
