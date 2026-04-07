# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
