# TTL and Rent

YieldVault keeps rent predictable by classifying every storage entry and
bounding how often TTL may be extended.

## Inventory

| Key | Durability | Tier | Notes |
| --- | --- | --- | --- |
| `Admin`, `Token`, `TotalShares`, `TotalAssets`, `MinDeposit`, `Paused`, `ExpectedWasmHash` | Instance | Hot config / aggregates | Share one instance TTL with the contract code/instance. |
| `Balance(user)` | Persistent | Hot user ledger | Extended when an active user is read or written. |
| `TtlInstanceBumped`, `TtlBalanceBumped(user)`, `TtlBumpCount` | Temporary | Per-ledger scratch | Dedup/budget only; values are scoped by ledger sequence. |

There are no cold durable entries today. Cleanup stays bounded because scratch
keys carry no business state and the bump budget caps host `extend_ttl` calls.

## Bump rules

Constants live in `src/storage.rs`:

- Instance: bump to `INSTANCE_BUMP_AMOUNT` (~30 days) when below
  `INSTANCE_LIFETIME_THRESHOLD` (~29 days), **at most once per ledger**.
- Persistent balances: same ~30 / ~29 day window, **at most once per user key
  per ledger** (a read followed by a write of the same balance shares one bump).
- Hard cap: `MAX_TTL_BUMPS_PER_INVOCATION` (8) bumps per ledger sequence.
  Further attempts no-op so rent work cannot grow unboundedly.

Dedup/budget scratch is keyed by ledger sequence, so activity in ledger *N*
never suppresses a legitimate bump in ledger *N+1*. Within one ledger, extra
bumps are redundant because TTL is measured in ledgers.

## Expiration behaviour

- **Instance archived:** the host rejects calls; no vault mutation is possible,
  so aggregates cannot corrupt.
- **Persistent balance archived / missing:** reads return `0`. Withdrawals fail
  with `InsufficientShares` and leave totals unchanged. A later deposit writes a
  fresh balance entry against the live aggregates.

## Tests

See `src/test.rs` cases prefixed `test_ttl_` for instrumentation of bump counts,
dedup across repeated reads, read/write coalescing, budget exhaustion, ledger
window reset, and expired-balance fail-safe withdrawals.
