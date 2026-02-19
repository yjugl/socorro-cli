# Contributing to socorro-cli

## Getting Started

```bash
git clone https://github.com/yjugl/socorro-cli.git
cd socorro-cli
cargo build
cargo test
cargo fmt
cargo clippy -- -D warnings
```

All tests must pass and clippy must report zero warnings before submitting changes.

## AI Tool Usage

This project was developed with AI assistance and follows Mozilla's [AI and Coding](https://firefox-source-docs.mozilla.org/contributing/ai-coding.html) policy. Key points:

- **Accountability**: You are accountable for all changes you submit, regardless of the tools you use.
- **Understanding**: You must understand and be able to explain every change you submit.
- **Quality**: Contributions must meet the same standards of correctness, security, and maintainability as any other patch.
- **Data protection**: Do not include private, security-sensitive, or otherwise confidential information in prompts to external AI tools.

## Data Privacy

socorro-cli accesses Mozilla's crash reporting systems (Socorro API, crash-pings.mozilla.org, correlations CDN). The tool is designed to process only **publicly available data** from crash reports, and its data models only deserialize public fields.

When contributing, follow these guidelines:

- Do not add features that process or store [protected crash report data](https://crash-stats.mozilla.org/documentation/protected_data_access/) such as minidumps, memory contents, user comments, email addresses, URLs from crash annotations, or exploitability ratings.
- The `ProcessedCrash` struct intentionally omits protected fields. Do not add protected data fields to this struct or to any other data model.
- New search columns or API fields should be limited to publicly available crash report fields (signatures, stack traces, module identifiers, product/version/platform metadata).
- If adding new API endpoints or data sources, document what data is accessed and confirm it is publicly available.

## License

This project is licensed under the [Mozilla Public License 2.0](LICENSE). All new `.rs` source files must include the MPL 2.0 header as the very first lines:

```rust
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
```
