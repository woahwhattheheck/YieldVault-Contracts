# ADR 0042: Version lifecycle event schemas

- Status: Accepted
- Deciders: YieldVault Contributors

## Context

Deposit, withdraw, and yield events previously published implicit tuples
(`(assets, shares)` / `(amount, total_assets)`) with no schema version, asset
id, post-state totals, or correlation key. Indexers could not safely evolve
when field meanings changed, and units were undocumented.

## Decision

Publish a versioned lifecycle schema:

- Topics: `(kind, EVENT_SCHEMA_VERSION, actor)`
- Data (v1): `(asset, amount_assets, amount_shares, total_assets, total_shares, correlation, outcome)`
- Units: underlying token base units and vault share units as `u128` (no hidden scale)
- Correlation: ledger sequence at emission
- Outcome: `"ok"` on successful emission (reverts emit nothing)
- Bump on-chain `VERSION` to 3 so consumers can detect the interface change
- Keep admin/control events (`init`, `paused`, `set_admin`, `upgrade`) on their
  existing compact payloads

Fixture-based parser tests decode the schema and fail closed on incompatible
versions and legacy shapes.

## Consequences

- Indexers gain an explicit migration path and can reject unknown schemas.
- Event payloads are larger (one extra topic + five data fields) — acceptable
  for the correctness gain.
- Existing consumers of the legacy tuples must update; `version() == 3` signals
  the break.
