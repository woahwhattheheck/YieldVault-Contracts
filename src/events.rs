//! Versioned event publishing helpers for the YieldVault.
//!
//! Lifecycle events (`deposit`, `withdraw`, `yield`) emit a documented schema
//! so indexers can evolve safely. Schema layout:
//!
//! ## Topics (all lifecycle events)
//!
//! `(kind: Symbol, schema_version: u32, actor: Address)`
//!
//! - `kind` — `"deposit"`, `"withdraw"`, or `"yield"`
//! - `schema_version` — [`types::EVENT_SCHEMA_VERSION`]; bump when field
//!   meanings or order change incompatibly
//! - `actor` — the authorizing party (depositor, withdrawer, or admin)
//!
//! ## Data payload (schema v1)
//!
//! `(asset, amount_assets, amount_shares, total_assets, total_shares, correlation, outcome)`
//!
//! | Field | Type | Units / meaning |
//! | --- | --- | --- |
//! | `asset` | `Address` | Underlying SEP-41 token contract |
//! | `amount_assets` | `u128` | Underlying token **base units** (no hidden scale) |
//! | `amount_shares` | `u128` | Vault share units (no hidden scale) |
//! | `total_assets` | `u128` | Vault aggregate assets after the mutation |
//! | `total_shares` | `u128` | Vault aggregate shares after the mutation |
//! | `correlation` | `u32` | Ledger sequence at emission (reconstruction key) |
//! | `outcome` | `Symbol` | `"ok"` on successful emission (reverts emit nothing) |
//!
//! Kind-specific semantics for the amount fields:
//! - **deposit** — `amount_assets` deposited, `amount_shares` minted
//! - **withdraw** — `amount_shares` burned, `amount_assets` redeemed
//! - **yield** — `amount_assets` credited as yield, `amount_shares` = 0
//!
//! Admin / control events (`init`, `paused`, `set_admin`, `upgrade`) keep their
//! existing compact payloads; they are not part of the versioned lifecycle
//! schema.

use soroban_sdk::{Address, BytesN, Env, Symbol};

use crate::types;

/// Outcome symbol published on every successful lifecycle event.
pub fn outcome_ok(env: &Env) -> Symbol {
    Symbol::new(env, "ok")
}

fn publish_lifecycle(
    env: &Env,
    kind: &str,
    actor: &Address,
    asset: &Address,
    amount_assets: u128,
    amount_shares: u128,
    total_assets: u128,
    total_shares: u128,
) {
    let topics = (
        Symbol::new(env, kind),
        types::EVENT_SCHEMA_VERSION,
        actor.clone(),
    );
    let correlation = env.ledger().sequence();
    let data = (
        asset.clone(),
        amount_assets,
        amount_shares,
        total_assets,
        total_shares,
        correlation,
        outcome_ok(env),
    );
    env.events().publish(topics, data);
}

/// Publishes a versioned `deposit` lifecycle event.
pub fn deposit(
    env: &Env,
    from: &Address,
    asset: &Address,
    assets: u128,
    shares: u128,
    total_assets: u128,
    total_shares: u128,
) {
    publish_lifecycle(
        env,
        "deposit",
        from,
        asset,
        assets,
        shares,
        total_assets,
        total_shares,
    );
}

/// Publishes a versioned `withdraw` lifecycle event.
pub fn withdraw(
    env: &Env,
    from: &Address,
    asset: &Address,
    shares: u128,
    assets: u128,
    total_assets: u128,
    total_shares: u128,
) {
    publish_lifecycle(
        env,
        "withdraw",
        from,
        asset,
        assets,
        shares,
        total_assets,
        total_shares,
    );
}

/// Publishes an `init` event recording the configured `admin` and underlying
/// `token` addresses.
pub fn initialize(env: &Env, admin: &Address, token: &Address) {
    let topics = (Symbol::new(env, "init"),);
    env.events().publish(topics, (admin.clone(), token.clone()));
}

/// Publishes a versioned `yield` lifecycle event.
pub fn accrue_yield(
    env: &Env,
    actor: &Address,
    asset: &Address,
    amount: u128,
    total_assets: u128,
    total_shares: u128,
) {
    publish_lifecycle(
        env,
        "yield",
        actor,
        asset,
        amount,
        0,
        total_assets,
        total_shares,
    );
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
