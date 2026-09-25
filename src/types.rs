//! Shared types and compile-time constants for the YieldVault contract.
//!
//! This module collects the storage [`DataKey`] enum together with the tunable
//! constants (mock APY, contract version, price scale) so they live in one
//! place and can be referenced consistently across the contract.

use soroban_sdk::{contracttype, Address};

/// The vault's default advertised APY, in basis points (500 == 5.00%).
pub const MOCK_APY_BPS: u32 = 500;

/// The on-chain contract version, bumped on each released interface change.
pub const VERSION: u32 = 3;

/// Fixed-point scale used when reporting the price of a single share, so that
/// fractional share prices survive integer division (1e9 == one whole asset).
pub const PRICE_SCALE: u128 = 1_000_000_000;

/// Number of basis points in 100% (10_000 bps == 100.00%), used when reporting
/// proportional figures such as an account's share of the vault and when
/// converting an annual yield rate into a simple-interest accrual factor.
pub const BPS_DENOMINATOR: u128 = 10_000;

/// Default minimum deposit applied when the vault is initialized, guarding
/// against dust deposits that would round down to zero shares.
pub const DEFAULT_MIN_DEPOSIT: u128 = 1;

/// Seconds in a non-leap year. Yield rates are annualized against this
/// denominator (simple interest: `assets * rate_bps * elapsed / (BPS * YEAR)`).
pub const SECONDS_PER_YEAR: u64 = 365 * 24 * 60 * 60;

/// Hard cap on the elapsed interval applied in a single accrual. Longer gaps
/// are clamped to this bound so a long inactive period cannot overflow the
/// accrual math or credit unbounded yield in one call.
pub const MAX_ACCRUAL_INTERVAL_SECS: u64 = SECONDS_PER_YEAR;

/// Maximum accepted annual yield rate in basis points (100_000 == 1000% APY).
/// Rejects absurd rates that would make accrual numerically unstable.
pub const MAX_YIELD_RATE_BPS: u32 = 100_000;

/// Keys used to address values in contract storage.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The vault administrator address (instance storage).
    Admin,
    /// The underlying asset token address (instance storage).
    Token,
    /// The total number of shares minted by the vault (instance storage).
    TotalShares,
    /// The total amount of underlying assets held by the vault (instance storage).
    TotalAssets,
    /// The minimum accepted deposit amount (instance storage).
    MinDeposit,
    /// Whether the vault is paused for new deposits (instance storage).
    Paused,
    /// A user's share balance, keyed by their address (persistent storage).
    Balance(Address),
    /// The admin-approved Wasm hash that the next upgrade must present
    /// (instance storage). Cleared automatically on a successful upgrade.
    ExpectedWasmHash,
    /// Annual yield rate in basis points (instance storage).
    YieldRateBps,
    /// Monotonic rate-configuration version; bumped on every successful
    /// [`crate::YieldVault::set_yield_rate`] (instance storage).
    YieldRateVersion,
    /// Unix ledger timestamp of the last successful accrual boundary
    /// (instance storage). Accrual is simple interest over
    /// `[LastAccruedAt, now]` clamped to [`MAX_ACCRUAL_INTERVAL_SECS`].
    LastAccruedAt,
}
