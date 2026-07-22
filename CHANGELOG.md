# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.5](https://github.com/okkez/clh-client/compare/v0.1.4...v0.1.5) - 2026-07-22

### Other

- *(deps)* update rust crate clap to v4.6.4 ([#72](https://github.com/okkez/clh-client/pull/72))
- *(deps)* update rust crate clap to v4.6.3 ([#71](https://github.com/okkez/clh-client/pull/71))
- *(deps)* update rust crate serde_json to v1.0.151 ([#70](https://github.com/okkez/clh-client/pull/70))
- *(deps)* update rust crate serde to v1.0.229 ([#69](https://github.com/okkez/clh-client/pull/69))
- *(deps)* update rust crate anyhow to v1.0.104 ([#68](https://github.com/okkez/clh-client/pull/68))
- *(deps)* update rust crate regex to v1.13.1 ([#65](https://github.com/okkez/clh-client/pull/65))
- *(deps)* update rust crate clap to v4.6.2 ([#64](https://github.com/okkez/clh-client/pull/64))
- *(deps)* update taiki-e/install-action action to v2.83.2
- *(deps)* update rust crate regex to v1.13.0 ([#62](https://github.com/okkez/clh-client/pull/62))
- *(deps)* update taiki-e/install-action action to v2.82.10
- *(deps)* update taiki-e/install-action action to v2.82.9
- *(deps)* update taiki-e/install-action action to v2.82.7
- *(deps)* update github actions
- *(deps)* update rust crate skim to v4.10.0 ([#56](https://github.com/okkez/clh-client/pull/56))
- *(deps)* update rust crate skim to v4.9.0 ([#55](https://github.com/okkez/clh-client/pull/55))
- *(deps)* update rust crate anyhow to v1.0.103 ([#54](https://github.com/okkez/clh-client/pull/54))
- *(deps)* update github actions
- *(deps)* update github actions to v2.82.2
- *(deps)* update github actions to v7
- *(deps)* update rust crate skim to v4.8.0 ([#49](https://github.com/okkez/clh-client/pull/49))
- *(deps)* update github actions
- *(deps)* update github actions
- *(deps)* update rust crate regex to v1.12.4 ([#47](https://github.com/okkez/clh-client/pull/47))
- *(deps)* update rust crate chrono to v0.4.45 ([#46](https://github.com/okkez/clh-client/pull/46))
- *(deps)* update github actions
- *(deps)* update rust crate skim to v4.7.0 ([#44](https://github.com/okkez/clh-client/pull/44))
- *(deps)* update rust crate serde_json to v1.0.150 ([#43](https://github.com/okkez/clh-client/pull/43))
- *(deps)* update rust crate skim to v4.6.3 ([#42](https://github.com/okkez/clh-client/pull/42))
- *(deps)* update actions/create-github-app-token action to v3.2.0
- *(deps)* update github actions
- *(deps)* update github actions
- *(deps)* update rust crate skim to v4.6.2 ([#38](https://github.com/okkez/clh-client/pull/38))
- *(deps)* update taiki-e/install-action action to v2.75.26
- *(deps)* update rust crate skim to v4.6.1 ([#36](https://github.com/okkez/clh-client/pull/36))
- *(deps)* update github actions
- *(deps)* update rust crate clap to v4.6.1 ([#34](https://github.com/okkez/clh-client/pull/34))
- *(deps)* update rust crate skim to v4.6.0 ([#33](https://github.com/okkez/clh-client/pull/33))
- *(deps)* update github actions
- *(deps)* update taiki-e/install-action action to v2.75.3
- *(deps)* update taiki-e/install-action action to v2.75.1

## [0.1.4](https://github.com/okkez/clh-client/compare/v0.1.3...v0.1.4) - 2026-04-07

### Added

- *(search)* wrap long commands in preview window to fit terminal width
- drop windows support

### Fixed

- *(search)* use is_some_and instead of map_or(false, ...) for clippy
- *(search)* address PR review comments on wrap_command

### Other

- *(deps)* update rust crate skim to v4.5.0 ([#26](https://github.com/okkez/clh-client/pull/26))
- cargo fmt

## [0.1.3](https://github.com/okkez/clh-client/compare/v0.1.2...v0.1.3) - 2026-03-22

### Other

- use GitHub App token in release-plz-release to trigger downstream workflows

## [0.1.2](https://github.com/okkez/clh-client/compare/v0.1.1...v0.1.2) - 2026-03-22

### Fixed

- trigger release workflow on published release event

## [0.1.1](https://github.com/okkez/clh-client/compare/v0.1.0...v0.1.1) - 2026-03-22

### Added

- make skim window height configurable via config file
- delete history record with Ctrl+D during search
- add ignore_patterns config for clh add

### Fixed

- return immediately on Enter/abort and delete only one record in global search
- stream skim results and always dedup to fix sort order and latency
- dereference Arc before downcasting SkimItem to HistoryItem
- enable HTTPS support in reqwest
- move AddConfig import to test module to remove unused import warning

### Other

- update README to reflect current keybindings and shell support
- move make_deduped_items to test scope to fix dead_code warning
- enforce strict Clippy lints and add nextest
- fix some warnings reported by clippy
- add Renovate config for automated dependency updates
