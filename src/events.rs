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

/// Publishes a `set_admin` event recording the transfer of the admin role from
/// `previous` to `new_admin`.
pub fn set_admin(env: &Env, previous: &Address, new_admin: &Address) {
    let topics = (Symbol::new(env, "set_admin"),);
    env.events()
        .publish(topics, (previous.clone(), new_admin.clone()));
}

/// Publishes an `upgrade` event recording that the contract's Wasm bytecode
/// was upgraded to `new_wasm_hash` by `admin`. The admin address is included
/// in the topics so indexers can attribute every upgrade to an authority.
pub fn upgrade(env: &Env, admin: &Address, new_wasm_hash: &BytesN<32>) {
    let topics = (Symbol::new(env, "upgrade"), admin.clone());
    env.events().publish(topics, new_wasm_hash.clone());
}

/// Publishes a `wd_limits` event recording the new per-operation and rolling-
/// period withdrawal caps (units: underlying assets) and the period length
/// in seconds.
pub fn withdraw_limits(env: &Env, max_per_op: u128, max_per_period: u128, period_secs: u64) {
    let topics = (Symbol::new(env, "wd_limits"),);
    env.events()
        .publish(topics, (max_per_op, max_per_period, period_secs));
}

/// Publishes a `wd_reset` event after an authorized rolling-period usage reset.
/// `cleared` is the withdrawn amount that was zeroed; `at` is the new period
/// start timestamp.
pub fn withdraw_period_reset(env: &Env, cleared: u128, at: u64) {
    let topics = (Symbol::new(env, "wd_reset"),);
    env.events().publish(topics, (cleared, at));
}

/// Publishes a `wd_override` event recording whether the emergency withdrawal-
/// limit override is enabled.
pub fn withdraw_override(env: &Env, enabled: bool) {
    let topics = (Symbol::new(env, "wd_override"),);
    env.events().publish(topics, enabled);
}
