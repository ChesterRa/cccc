# Changelog

All notable changes to this project are documented in this file.

The format follows Keep a Changelog, and versions follow SemVer/PEP 440 release tags used by this repo.

## [Unreleased]

### Added
- RC19 release program documentation under `docs/release/`.
- RC19 matrix tooling under `scripts/release/`.
- MCP parity support: `cccc_group_set_state` now accepts `stopped` (mapped to `group_stop`).
- Test coverage for MCP stopped-state mapping.

### Changed
- Release and standards docs aligned to version-agnostic examples where appropriate.
- CLI/reference docs aligned to actual command surface.
- CI and release workflows now run `pytest`.

## [0.4.0rc18]

### Notes
- Release candidate baseline before rc19 quality-convergence cycle.
