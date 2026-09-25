//! Storage accessors for the YieldVault contract.
//!
//! Configuration and aggregate totals live in *instance* storage, which shares
//! its time-to-live with the contract instance itself. Per-user share balances
//! live in *persistent* storage and are extended on every read and write so
//! that active users do not have their balances archived.

use soroban_sdk::{Address, BytesN, Env};

use crate::error::Error;
use crate::types::DataKey;

/// Number of ledgers in roughly one day (assuming ~5 second ledgers).
const DAY_IN_LEDGERS: u32 = 17280;

/// Time-to-live bump amount for instance storage entries.
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
/// Threshold at which instance storage entries are extended.
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Time-to-live bump amount for persistent storage entries.
const PERSISTENT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
/// Threshold at which persistent storage entries are extended.
const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extend the time-to-live of the instance storage so the contract stays live.
pub fn extend_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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
pub fn get_balance(env: &Env, user: &Address) -> u128 {
    let key = DataKey::Balance(user.clone());
    let balance = env.storage().persistent().get(&key).unwrap_or(0);
    if env.storage().persistent().has(&key) {
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }
    balance
}

/// Writes a user's share balance to persistent storage.
pub fn set_balance(env: &Env, user: &Address, balance: u128) {
    let key = DataKey::Balance(user.clone());
    env.storage().persistent().set(&key, &balance);
    env.storage().persistent().extend_ttl(
        &key,
        PERSISTENT_LIFETIME_THRESHOLD,
        PERSISTENT_BUMP_AMOUNT,
    );
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
