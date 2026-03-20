# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/okkez/clh-client/compare/v0.1.0...v0.1.1) - 2026-03-20

### Added

- add ignore_patterns config for clh add

### Fixed

- add missing id-token write permission to release-plz workflow
- move AddConfig import to test module to remove unused import warning

### Other

- add missing environemnt
- use crates-io-auth-action for release job token
- fix some warnings reported by clippy
- add Renovate config for automated dependency updates
