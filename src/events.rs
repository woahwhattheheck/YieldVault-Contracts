//! Event publishing helpers for the YieldVault.
//!
//! Each helper publishes a topic identifying the event kind (and, where
//! relevant, the affected account) together with a data payload of the amounts
//! involved, so off-chain indexers can track vault activity.

use soroban_sdk::{Address, BytesN, Env, Symbol};

/// Publishes a `deposit` event recording that `from` supplied `assets` of the
/// underlying token in exchange for `shares` vault shares.
pub fn deposit(env: &Env, from: &Address, assets: u128, shares: u128) {
    let topics = (Symbol::new(env, "deposit"), from.clone());
    env.events().publish(topics, (assets, shares));
}

/// Publishes a `withdraw` event recording that `from` burned `shares` vault
/// shares to redeem `assets` of the underlying token.
pub fn withdraw(env: &Env, from: &Address, shares: u128, assets: u128) {
    let topics = (Symbol::new(env, "withdraw"), from.clone());
    env.events().publish(topics, (shares, assets));
}

/// Publishes an `init` event recording the configured `admin` and underlying
/// `token` addresses.
pub fn initialize(env: &Env, admin: &Address, token: &Address) {
    let topics = (Symbol::new(env, "init"),);
    env.events().publish(topics, (admin.clone(), token.clone()));
}

/// Publishes a `yield` event recording the `amount` of assets accrued to the
/// vault as mock yield, alongside the new total assets figure.
pub fn accrue_yield(env: &Env, amount: u128, total_assets: u128) {
    let topics = (Symbol::new(env, "yield"),);
    env.events().publish(topics, (amount, total_assets));
}

/// Publishes a `paused` event recording the vault's new paused state, so
/// indexers can track when deposits are halted or resumed.
pub fn paused(env: &Env, paused: bool) {
    let topics = (Symbol::new(env, "paused"),);
    env.events().publish(topics, paused);
}

/// Publishes an `admin_proposed` event recording that `current` proposed
/// `pending` as the next administrator, with acceptance deadline `expires_at`.
pub fn admin_proposed(env: &Env, current: &Address, pending: &Address, expires_at: u64) {
    let topics = (Symbol::new(env, "admin_proposed"),);
    env.events()
        .publish(topics, (current.clone(), pending.clone(), expires_at));
}

/// Publishes an `admin_accepted` event recording that `previous` handed the
/// admin role to `new_admin` after an explicit acceptance.
pub fn admin_accepted(env: &Env, previous: &Address, new_admin: &Address) {
    let topics = (Symbol::new(env, "admin_accepted"),);
    env.events()
        .publish(topics, (previous.clone(), new_admin.clone()));
}

/// Publishes an `admin_proposal_cancelled` event recording that `admin`
/// cancelled a pending rotation to `pending`.
pub fn admin_proposal_cancelled(env: &Env, admin: &Address, pending: &Address) {
    let topics = (Symbol::new(env, "admin_cancelled"),);
    env.events()
        .publish(topics, (admin.clone(), pending.clone()));
}

/// Publishes an `upgrade` event recording that the contract's Wasm bytecode
/// was upgraded to `new_wasm_hash` by `admin`. The admin address is included
/// in the topics so indexers can attribute every upgrade to an authority.
pub fn upgrade(env: &Env, admin: &Address, new_wasm_hash: &BytesN<32>) {
    let topics = (Symbol::new(env, "upgrade"), admin.clone());
    env.events().publish(topics, new_wasm_hash.clone());
}
