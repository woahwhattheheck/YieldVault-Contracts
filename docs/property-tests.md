# Property Tests

Deterministic fuzz / property coverage for the YieldVault math and state
transitions lives in `src/fuzz.rs` (issue #74).

## Harness

- Fixed seed: `fuzz::FUZZ_SEED` (`0x0059_5646_3734_2026`).
- PRNG: Numerical Recipes LCG; same seed → same inputs in CI.
- On failure, assertions include `seed`, label, and minimized inputs so a
  regression fixture can be cut without re-searching.

## Invariants covered

| Suite | Property |
| --- | --- |
| `fuzz_mul_div_floor_or_safe_reject` | Floored `a*b/d`, or `DivisionByZero` / `MathOverflow` |
| `fuzz_convert_round_trip_conserves_value` | assets→shares→assets never mints value |
| `fuzz_convert_shares_round_trip_conserves` | shares→assets→shares never inflates shares |
| `fuzz_convert_to_shares_monotonic` | more assets ⇒ ≥ shares (fixed vault state) |
| `fuzz_convert_to_assets_monotonic` | more shares ⇒ ≥ assets |
| `fuzz_share_fraction_bps_bounded` | holder ≤ total shares ⇒ fraction ≤ `bps` (fee/rate bound) |
| `fuzz_price_per_share_monotonic_in_assets` | more assets ⇒ ≥ price per share |
| `fuzz_empty_vault_bootstrap_and_zero_totals` | empty-vault 1:1 bootstrap / zero redeem |
| `fuzz_regression_fixtures_overflow_and_bounds` | pinned overflow, floor, and 100% BPS edges |
| `fuzz_vault_deposit_withdraw_conserves_under_yield` | full redeem ≤ deposit + yield (solo depositor) |

No invariant is weakened to obtain a green run. Existing example-based tests
in `src/test.rs` remain the readable regression layer alongside this suite.

See the README and the sources under `src/` for the authoritative implementation.
