//! Shared types and compile-time constants for the YieldVault contract.
//!
//! This module collects the storage [`DataKey`] enum together with the tunable
//! constants (mock APY, contract version, price scale) so they live in one
//! place and can be referenced consistently across the contract.

use soroban_sdk::{contracttype, Address};

/// The vault's advertised mock APY, in basis points (500 == 5.00%).
pub const MOCK_APY_BPS: u32 = 500;

/// The on-chain contract version, bumped on each released interface change.
pub const VERSION: u32 = 3;

/// Fixed-point scale used when reporting the price of a single share, so that
/// fractional share prices survive integer division (1e9 == one whole asset).
pub const PRICE_SCALE: u128 = 1_000_000_000;

/// Number of basis points in 100% (10_000 bps == 100.00%), used when reporting
/// proportional figures such as an account's share of the vault.
pub const BPS_DENOMINATOR: u128 = 10_000;

/// Default minimum deposit applied when the vault is initialized, guarding
/// against dust deposits that would round down to zero shares.
pub const DEFAULT_MIN_DEPOSIT: u128 = 1;

/// How long an admin-rotation proposal remains acceptable, in seconds
/// (7 days). After this window `accept_admin` rejects the proposal and the
/// active administrator is unchanged.
pub const ADMIN_PROPOSAL_TTL_SECS: u64 = 7 * 24 * 60 * 60;

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
    /// Pending admin address awaiting acceptance (instance storage).
    PendingAdmin,
    /// Unix timestamp (ledger time) when the pending admin proposal expires
    /// (instance storage).
    AdminProposalExpiry,
    /// The admin-approved Wasm hash that the next upgrade must present
    /// (instance storage). Cleared automatically on a successful upgrade.
    ExpectedWasmHash,
}
