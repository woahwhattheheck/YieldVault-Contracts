# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Bounded simple-interest yield accrual with explicit rate (bps), ledger-timestamp
  monotonicity, max accrual interval clamp, and rate-versioned `yield` / `rate`
  events (`get_last_accrued_at`, `get_yield_rate`, `get_yield_rate_version`,
  `set_yield_rate`). Closes #70.

### Changed

- `accrue_yield` no longer takes a free-form amount; yield is derived from
  `total_assets * rate_bps * elapsed / (BPS * SECONDS_PER_YEAR)`. On-chain
  `version()` bumped to 3.
- Aggregate totals (`total_shares`, `total_assets`, user balances) now use
  saturating arithmetic (`saturating_add`/`saturating_sub`) instead of
  checked arithmetic, per ADR 0026. Overflow caps at `u128::MAX` and
  underflow floors at `0` rather than returning `Error::MathOverflow`.
  The internal `mul_div` helper in `math.rs` continues to use checked
  multiplication for intermediate products.

### Added

- `scripts/verify_wasm_hash.sh` — a Bash script that computes the SHA-256 of
  the local WASM artefact and compares it against the on-chain hash retrieved
  via `stellar contract info`.  Exits 0 on match, 1 on mismatch, 2 on any
  pre-condition failure, making it safe to use in CI pipelines.
- `make verify-hash CONTRACT_ID=<id>` Makefile convenience target.
- `make test-scripts` Makefile target that runs the bash test suite in
  `tests/test_verify_wasm_hash.sh`.
- `tests/test_verify_wasm_hash.sh` — hermetic bash test suite for the
  verification script (uses a stub `stellar` binary; no live network needed).
- ADR 0041 documenting the decision to add the verification script.
- Expanded `docs/deployment-guide.md` with WASM hash verification steps.
- Updated `docs/mainnet-checklist.md` to include hash verification as a
  mandatory pre-launch step.

## [0.2.0]

### Added

- `preview_deposit` and `preview_withdraw` view getters (ERC4626-style aliases).
- `max_redeem` view returning a user's redeemable share balance.
- `share_percentage` view reporting a user's share of the vault in basis points.
- Configurable minimum deposit with the `BelowMinimumDeposit` error and an
  admin `set_min_deposit` setter plus a `get_min_deposit` getter.
- Admin pause control (`set_paused` / `is_paused`) guarding deposits, the
  `Paused` error, and a `paused` event.
- Admin role transfer via `set_admin`, emitting a `set_admin` event.

## [0.1.0]

### Added

- Initial share-based (ERC4626-style) yield vault: deposit, withdraw, mock
  yield accrual, share/asset conversion views, events, and error codes.
