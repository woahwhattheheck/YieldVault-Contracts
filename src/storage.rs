//! Storage accessors for the YieldVault contract.
//!
//! # Storage inventory and TTL policy
//!
//! | Entry | Tier | Durability | Bump rule |
//! | --- | --- | --- | --- |
//! | `Admin`, `Token`, `TotalShares`, `TotalAssets`, `MinDeposit`, `Paused`, `ExpectedWasmHash` | hot config / aggregates | **instance** | One shared instance TTL; bumped **at most once per ledger** via [`extend_instance`]. |
//! | `Balance(user)` | hot user ledger | **persistent** | Bumped **at most once per user key per ledger** on read (if present) or write. |
//!
//! There are currently no cold / temporary business-data entries. Temporary
//! storage holds only per-ledger TTL bump dedup flags and the bump budget
//! counter (see below).
//!
//! ## Budgets
//!
//! - Instance bump target: [`INSTANCE_BUMP_AMOUNT`] ledgers (~30 days), threshold
//!   [`INSTANCE_LIFETIME_THRESHOLD`] (~29 days).
//! - Persistent bump target: [`PERSISTENT_BUMP_AMOUNT`] ledgers (~30 days),
//!   threshold [`PERSISTENT_LIFETIME_THRESHOLD`] (~29 days).
//! - Per-ledger cap: [`MAX_TTL_BUMPS_PER_INVOCATION`]. Once the cap is reached,
//!   further bump attempts become no-ops so rent work stays bounded. Existing
//!   TTLs remain in force.
//!
//! Dedup and budget scratch keys are scoped by **ledger sequence**, so a bump
//! in ledger *N* never suppresses a legitimate bump in ledger *N+1*. Within a
//! single ledger, repeated reads (or a read followed by a write of the same
//! key) share one `extend_ttl` — further bumps in that ledger are redundant
//! because TTL is measured in ledgers.
//!
//! ## Expiration behaviour
//!
//! - **Instance expired / archived:** the contract instance is inaccessible;
//!   every entrypoint fails closed at the host layer. Accounting cannot drift
//!   because no mutation is possible.
//! - **Persistent `Balance(user)` expired / archived / missing:** reads treat
//!   the balance as `0`. Withdrawals against a zero balance return
//!   [`Error::InsufficientShares`] and leave `TotalShares` / `TotalAssets`
//!   unchanged. Deposits mint against the live aggregates and write a fresh
//!   balance entry. Expired user ledgers therefore cannot invent shares or
//!   drain assets.

use soroban_sdk::{Address, BytesN, Env};

use crate::error::Error;
use crate::types::DataKey;

/// Number of ledgers in roughly one day (assuming ~5 second ledgers).
pub const DAY_IN_LEDGERS: u32 = 17280;

/// Time-to-live bump amount for instance storage entries (~30 days).
pub const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
/// Threshold at which instance storage entries are extended (~29 days).
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Time-to-live bump amount for persistent storage entries (~30 days).
pub const PERSISTENT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
/// Threshold at which persistent storage entries are extended (~29 days).
pub const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Hard cap on `extend_ttl` calls issued by this contract for a single ledger
/// sequence (instance + all persistent keys combined).
///
/// Sized for representative workloads (one instance bump + a handful of user
/// balance touches) with headroom, while keeping rent work bounded.
pub const MAX_TTL_BUMPS_PER_INVOCATION: u32 = 8;

/// Extend the time-to-live of the instance storage so the contract stays live.
///
/// Deduped and budgeted: at most one instance bump per ledger sequence, and
/// only if the per-ledger bump budget still has capacity.
pub fn extend_instance(env: &Env) {
    if instance_already_bumped(env) {
        return;
    }
    if !consume_bump_budget(env) {
        return;
    }
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    mark_instance_bumped(env);
}

/// Returns `true` if the vault has already been initialized.
pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

/// Returns [`Error::NotInitialized`] unless the vault has been initialized.
pub fn require_initialized(env: &Env) -> Result<(), Error> {
    if has_admin(env) {
        Ok(())
    } else {
        Err(Error::NotInitialized)
    }
}

/// Reads the admin address from instance storage.
pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("admin not set")
}

/// Writes the admin address to instance storage.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Reads the underlying token address from instance storage.
pub fn get_token(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Token)
        .expect("token not set")
}

/// Writes the underlying token address to instance storage.
pub fn set_token(env: &Env, token: &Address) {
    env.storage().instance().set(&DataKey::Token, token);
}

/// Reads the total number of shares minted, defaulting to zero.
pub fn get_total_shares(env: &Env) -> u128 {
    env.storage()
        .instance()
        .get(&DataKey::TotalShares)
        .unwrap_or(0)
}

/// Writes the total number of shares minted.
pub fn set_total_shares(env: &Env, shares: u128) {
    env.storage().instance().set(&DataKey::TotalShares, &shares);
}

/// Reads the total amount of underlying assets held, defaulting to zero.
pub fn get_total_assets(env: &Env) -> u128 {
    env.storage()
        .instance()
        .get(&DataKey::TotalAssets)
        .unwrap_or(0)
}

/// Writes the total amount of underlying assets held.
pub fn set_total_assets(env: &Env, assets: u128) {
    env.storage().instance().set(&DataKey::TotalAssets, &assets);
}

/// Reads the minimum accepted deposit, defaulting to
/// [`crate::types::DEFAULT_MIN_DEPOSIT`] when unset.
pub fn get_min_deposit(env: &Env) -> u128 {
    env.storage()
        .instance()
        .get(&DataKey::MinDeposit)
        .unwrap_or(crate::types::DEFAULT_MIN_DEPOSIT)
}

/// Writes the minimum accepted deposit amount.
pub fn set_min_deposit(env: &Env, amount: u128) {
    env.storage().instance().set(&DataKey::MinDeposit, &amount);
}

/// Reads whether the vault is paused, defaulting to `false` (active).
pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

/// Writes the vault's paused flag.
pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

/// Reads a user's share balance from persistent storage, defaulting to zero.
///
/// Missing or archived entries fail safe as `0` (see module docs). When the
/// entry is present its TTL is extended at most once per ledger sequence.
pub fn get_balance(env: &Env, user: &Address) -> u128 {
    let key = DataKey::Balance(user.clone());
    let balance = env.storage().persistent().get(&key).unwrap_or(0);
    if env.storage().persistent().has(&key) {
        bump_persistent(env, user);
    }
    balance
}

/// Writes a user's share balance to persistent storage and extends its TTL at
/// most once per ledger sequence (deduped against a prior [`get_balance`] bump).
pub fn set_balance(env: &Env, user: &Address, balance: u128) {
    let key = DataKey::Balance(user.clone());
    env.storage().persistent().set(&key, &balance);
    bump_persistent(env, user);
}

/// Returns the admin-approved expected Wasm hash, if one has been staged.
pub fn get_expected_wasm_hash(env: &Env) -> Option<BytesN<32>> {
    env.storage().instance().get(&DataKey::ExpectedWasmHash)
}

/// Stores the admin-approved Wasm hash that the next upgrade must present.
pub fn set_expected_wasm_hash(env: &Env, hash: &BytesN<32>) {
    env.storage()
        .instance()
        .set(&DataKey::ExpectedWasmHash, hash);
}

/// Removes the staged expected Wasm hash after a successful upgrade.
pub fn clear_expected_wasm_hash(env: &Env) {
    env.storage().instance().remove(&DataKey::ExpectedWasmHash);
}

/// Number of TTL bumps performed so far for the current ledger sequence.
///
/// Exposed for storage-instrumentation tests and operators probing rent spend.
pub fn ttl_bump_count(env: &Env) -> u32 {
    let seq = env.ledger().sequence();
    match env
        .storage()
        .temporary()
        .get::<_, (u32, u32)>(&DataKey::TtlBumpCount)
    {
        Some((recorded_seq, count)) if recorded_seq == seq => count,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Internal TTL budget / dedup helpers (ledger-scoped)
// ---------------------------------------------------------------------------

fn instance_already_bumped(env: &Env) -> bool {
    let seq = env.ledger().sequence();
    matches!(
        env.storage()
            .temporary()
            .get::<_, u32>(&DataKey::TtlInstanceBumped),
        Some(recorded) if recorded == seq
    )
}

fn mark_instance_bumped(env: &Env) {
    env.storage()
        .temporary()
        .set(&DataKey::TtlInstanceBumped, &env.ledger().sequence());
}

fn balance_already_bumped(env: &Env, user: &Address) -> bool {
    let seq = env.ledger().sequence();
    matches!(
        env.storage()
            .temporary()
            .get::<_, u32>(&DataKey::TtlBalanceBumped(user.clone())),
        Some(recorded) if recorded == seq
    )
}

fn mark_balance_bumped(env: &Env, user: &Address) {
    env.storage().temporary().set(
        &DataKey::TtlBalanceBumped(user.clone()),
        &env.ledger().sequence(),
    );
}

/// Attempt to consume one unit of the per-ledger bump budget.
///
/// Returns `false` when the budget is exhausted so callers can skip the host
/// `extend_ttl` rather than unbounded-extend rent.
fn consume_bump_budget(env: &Env) -> bool {
    let used = ttl_bump_count(env);
    if used >= MAX_TTL_BUMPS_PER_INVOCATION {
        return false;
    }
    let seq = env.ledger().sequence();
    env.storage()
        .temporary()
        .set(&DataKey::TtlBumpCount, &(seq, used + 1));
    true
}

fn bump_persistent(env: &Env, user: &Address) {
    if balance_already_bumped(env, user) {
        return;
    }
    if !consume_bump_budget(env) {
        return;
    }
    let key = DataKey::Balance(user.clone());
    env.storage().persistent().extend_ttl(
        &key,
        PERSISTENT_LIFETIME_THRESHOLD,
        PERSISTENT_BUMP_AMOUNT,
    );
    mark_balance_bumped(env, user);
}
