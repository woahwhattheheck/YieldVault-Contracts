#![no_std]

//! # YieldVault
//!
//! A share-based (ERC4626-style) DeFi yield vault for the Soroban platform.
//!
//! Depositors supply an underlying token and receive vault shares that track
//! their proportional claim on the vault's assets. As the vault accrues yield
//! the value of each share grows, so withdrawing the same number of shares
//! later returns more of the underlying token.

mod error;
mod events;
mod math;
mod storage;
mod types;

#[cfg(test)]
mod test;

pub use error::Error;

use soroban_sdk::{contract, contractimpl, contractmeta, token, Address, BytesN, Env};

contractmeta!(
    key = "Description",
    val = "Share-based ERC4626-style yield vault"
);

/// The YieldVault contract type.
#[contract]
pub struct YieldVault;

/// Convert a vault `u128` amount into the `i128` expected by SEP-41 token clients.
fn amount_to_i128(amount: u128) -> Result<i128, Error> {
    i128::try_from(amount).map_err(|_| Error::AmountOverflow)
}

/// Ensure a single user's share balance never exceeds the vault-wide total.
fn ensure_share_invariant(env: &Env, user: &Address) -> Result<(), Error> {
    if storage::get_balance(env, user) > storage::get_total_shares(env) {
        return Err(Error::InvariantViolation);
    }
    Ok(())
}

/// Pull `amount` of `token` from `from` into the vault and require an exact
/// balance increase. Transfer failures and short/malformed deliveries become
/// errors so the host rolls back every related vault mutation.
fn pull_token_exact(env: &Env, token: &Address, from: &Address, amount: u128) -> Result<(), Error> {
    let amount_i = amount_to_i128(amount)?;
    let client = token::Client::new(env, token);
    let vault = env.current_contract_address();
    let before = client.balance(&vault);
    match client.try_transfer(from, &vault, &amount_i) {
        Ok(Ok(())) => {}
        _ => return Err(Error::TokenTransferFailed),
    }
    let after = client.balance(&vault);
    let received = after
        .checked_sub(before)
        .ok_or(Error::TransferAmountMismatch)?;
    if received != amount_i {
        return Err(Error::TransferAmountMismatch);
    }
    Ok(())
}

/// Push `amount` of `token` from the vault to `to` and require an exact
/// balance decrease. Transfer failures and short/malformed deliveries become
/// errors so the host rolls back every related vault mutation (including prior
/// share burns).
fn push_token_exact(env: &Env, token: &Address, to: &Address, amount: u128) -> Result<(), Error> {
    let amount_i = amount_to_i128(amount)?;
    let client = token::Client::new(env, token);
    let vault = env.current_contract_address();
    let before = client.balance(&vault);
    match client.try_transfer(&vault, to, &amount_i) {
        Ok(Ok(())) => {}
        _ => return Err(Error::TokenTransferFailed),
    }
    let after = client.balance(&vault);
    let sent = before
        .checked_sub(after)
        .ok_or(Error::TransferAmountMismatch)?;
    if sent != amount_i {
        return Err(Error::TransferAmountMismatch);
    }
    Ok(())
}

#[contractimpl]
impl YieldVault {
    /// Initializes the vault with its `admin` and the `token` it accepts as the
    /// underlying asset.
    ///
    /// Can only be called once; a second call returns
    /// [`Error::AlreadyInitialized`].
    pub fn initialize(env: Env, admin: Address, token: Address) -> Result<(), Error> {
        if storage::has_admin(&env) {
            return Err(Error::AlreadyInitialized);
        }
        storage::set_admin(&env, &admin);
        storage::set_token(&env, &token);
        // Seed the configurable defaults so their stored values are explicit
        // from the outset rather than relying solely on read-time fallbacks.
        storage::set_min_deposit(&env, types::DEFAULT_MIN_DEPOSIT);
        storage::set_paused(&env, false);
        storage::extend_instance(&env);
        events::initialize(&env, &admin, &token);
        Ok(())
    }

    /// Returns `true` if the vault has been initialized.
    ///
    /// Unlike the other getters this never errors, so callers can probe the
    /// vault's setup state without handling [`Error::NotInitialized`].
    pub fn is_initialized(env: Env) -> bool {
        storage::has_admin(&env)
    }

    /// Returns the vault administrator address.
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        storage::require_initialized(&env)?;
        Ok(storage::get_admin(&env))
    }

    /// Transfers the admin role to `new_admin`.
    ///
    /// Admin-only: requires authorization from the current admin. Emits a
    /// `set_admin` event recording the previous and new admin addresses.
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let current = storage::get_admin(&env);
        current.require_auth();

        storage::set_admin(&env, &new_admin);
        storage::extend_instance(&env);
        events::set_admin(&env, &current, &new_admin);
        Ok(())
    }

    /// Returns the underlying asset token address.
    pub fn get_token(env: Env) -> Result<Address, Error> {
        storage::require_initialized(&env)?;
        Ok(storage::get_token(&env))
    }

    /// Returns the total number of shares minted by the vault.
    pub fn total_shares(env: Env) -> u128 {
        storage::get_total_shares(&env)
    }

    /// Returns the total amount of underlying assets held by the vault.
    pub fn total_assets(env: Env) -> u128 {
        storage::get_total_assets(&env)
    }

    /// Returns the share balance of `user`.
    pub fn balance_of(env: Env, user: Address) -> u128 {
        storage::get_balance(&env, &user)
    }

    /// Previews how many shares would be minted for depositing `assets` at the
    /// current exchange rate.
    pub fn convert_to_shares(env: Env, assets: u128) -> Result<u128, Error> {
        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        math::convert_to_shares(assets, total_shares, total_assets)
    }

    /// Previews how many shares a [`Self::deposit`] of `assets` would mint at
    /// the current exchange rate, without modifying any state.
    ///
    /// This is the ERC4626-style alias for [`Self::convert_to_shares`], provided
    /// so integrators can use the conventional preview naming.
    pub fn preview_deposit(env: Env, assets: u128) -> Result<u128, Error> {
        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        math::convert_to_shares(assets, total_shares, total_assets)
    }

    /// Returns the value of a single share in underlying assets, scaled by
    /// [`types::PRICE_SCALE`] to preserve fractional precision.
    ///
    /// An empty vault reports a price of exactly one whole asset per share.
    pub fn price_per_share(env: Env) -> Result<u128, Error> {
        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        math::price_per_share(total_shares, total_assets, types::PRICE_SCALE)
    }

    /// Previews how many underlying assets `shares` would redeem for at the
    /// current exchange rate.
    pub fn convert_to_assets(env: Env, shares: u128) -> Result<u128, Error> {
        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        math::convert_to_assets(shares, total_shares, total_assets)
    }

    /// Previews how many underlying assets a [`Self::withdraw`] of `shares`
    /// would return at the current exchange rate, without modifying any state.
    ///
    /// This is the ERC4626-style alias for [`Self::convert_to_assets`], provided
    /// so integrators can use the conventional preview naming.
    pub fn preview_withdraw(env: Env, shares: u128) -> Result<u128, Error> {
        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        math::convert_to_assets(shares, total_shares, total_assets)
    }

    /// Returns the amount of underlying assets `user` could withdraw by
    /// redeeming their entire share balance at the current exchange rate.
    pub fn max_withdraw(env: Env, user: Address) -> Result<u128, Error> {
        let shares = storage::get_balance(&env, &user);
        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        math::convert_to_assets(shares, total_shares, total_assets)
    }

    /// Returns the smallest deposit the vault currently accepts.
    pub fn get_min_deposit(env: Env) -> u128 {
        storage::get_min_deposit(&env)
    }

    /// Returns `true` if the vault is paused for new deposits.
    pub fn is_paused(env: Env) -> bool {
        storage::is_paused(&env)
    }

    /// Pauses or resumes the vault's acceptance of new deposits.
    ///
    /// Withdrawals remain available while paused so depositors can always exit.
    /// Admin-only: requires authorization from the configured admin address.
    pub fn set_paused(env: Env, paused: bool) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let admin = storage::get_admin(&env);
        admin.require_auth();

        storage::set_paused(&env, paused);
        storage::extend_instance(&env);
        events::paused(&env, paused);
        Ok(())
    }

    /// Updates the minimum accepted deposit amount.
    ///
    /// Admin-only: requires authorization from the configured admin address.
    pub fn set_min_deposit(env: Env, amount: u128) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let admin = storage::get_admin(&env);
        admin.require_auth();

        storage::set_min_deposit(&env, amount);
        storage::extend_instance(&env);
        Ok(())
    }

    /// Returns the fraction of the vault owned by `user`, expressed in basis
    /// points (`10_000` bps == 100%).
    ///
    /// Reports zero for an empty vault and rounds down, so the figure never
    /// overstates a user's claim on the vault's assets.
    pub fn share_percentage(env: Env, user: Address) -> Result<u128, Error> {
        let shares = storage::get_balance(&env, &user);
        let total_shares = storage::get_total_shares(&env);
        math::share_fraction_bps(shares, total_shares, types::BPS_DENOMINATOR)
    }

    /// Returns the maximum number of shares `user` can redeem, which is simply
    /// their current share balance.
    ///
    /// Provided as the ERC4626-style counterpart to [`Self::max_withdraw`],
    /// which reports the same position denominated in underlying assets.
    pub fn max_redeem(env: Env, user: Address) -> u128 {
        storage::get_balance(&env, &user)
    }

    /// Deposits `amount` of the underlying token from `from` into the vault,
    /// minting and returning the number of shares credited to `from`.
    ///
    /// Requires authorization from `from`. Token movement and vault accounting
    /// are atomic: the pull must credit the vault by exactly `amount`, then
    /// shares/assets/balances are updated once. Transfer failures, short or
    /// malformed token deliveries, and invariant violations return an error so
    /// the host rolls back every related mutation.
    pub fn deposit(env: Env, from: Address, amount: u128) -> Result<u128, Error> {
        storage::require_initialized(&env)?;
        from.require_auth();

        if storage::is_paused(&env) {
            return Err(Error::Paused);
        }
        if amount == 0 {
            return Err(Error::ZeroAmount);
        }
        if amount < storage::get_min_deposit(&env) {
            return Err(Error::BelowMinimumDeposit);
        }

        // Reject amounts the SEP-41 token interface cannot represent before any
        // state change or external call.
        let _ = amount_to_i128(amount)?;

        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        let shares = math::convert_to_shares(amount, total_shares, total_assets)?;
        if shares == 0 {
            return Err(Error::ZeroShares);
        }

        // Interaction: pull tokens and require an exact balance increase.
        let token_address = storage::get_token(&env);
        pull_token_exact(&env, &token_address, &from, amount)?;

        // Effects: mint shares and credit assets exactly once.
        let new_total_shares = total_shares.saturating_add(shares);
        let new_total_assets = total_assets.saturating_add(amount);
        let user_balance = storage::get_balance(&env, &from).saturating_add(shares);

        storage::set_total_shares(&env, new_total_shares);
        storage::set_total_assets(&env, new_total_assets);
        storage::set_balance(&env, &from, user_balance);
        ensure_share_invariant(&env, &from)?;
        storage::extend_instance(&env);

        events::deposit(&env, &from, amount, shares);
        Ok(shares)
    }

    /// Burns `shares` from `from` and returns the corresponding amount of
    /// underlying assets, transferring them back to `from`.
    ///
    /// Requires authorization from `from`. Token movement and vault accounting
    /// are atomic (checks-effects-interactions): shares and aggregates are
    /// updated first, then the outbound transfer must debit the vault by
    /// exactly the redeemed asset amount. Transfer failures or malformed
    /// deliveries return an error so the host rolls back the share burn and
    /// every related mutation.
    ///
    /// Returns [`Error::InsufficientShares`] if `from` does not hold enough
    /// shares.
    pub fn withdraw(env: Env, from: Address, shares: u128) -> Result<u128, Error> {
        storage::require_initialized(&env)?;
        from.require_auth();

        if shares == 0 {
            return Err(Error::ZeroShares);
        }

        let user_balance = storage::get_balance(&env, &from);
        if user_balance < shares {
            return Err(Error::InsufficientShares);
        }

        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        let assets = math::convert_to_assets(shares, total_shares, total_assets)?;
        if assets == 0 {
            return Err(Error::ZeroAmount);
        }

        // Reject amounts the SEP-41 token interface cannot represent before any
        // state change or external call.
        let _ = amount_to_i128(assets)?;

        // Effects before interaction so a re-entering token cannot observe
        // stale balances. A failed/malformed push rolls these effects back.
        let new_total_shares = total_shares.saturating_sub(shares);
        let new_total_assets = total_assets.saturating_sub(assets);
        let new_user_balance = user_balance.saturating_sub(shares);

        storage::set_total_shares(&env, new_total_shares);
        storage::set_total_assets(&env, new_total_assets);
        storage::set_balance(&env, &from, new_user_balance);
        ensure_share_invariant(&env, &from)?;

        let token_address = storage::get_token(&env);
        push_token_exact(&env, &token_address, &from, assets)?;

        storage::extend_instance(&env);
        events::withdraw(&env, &from, shares, assets);
        Ok(assets)
    }

    /// Mocks yield accrual by increasing the vault's total assets by `amount`
    /// without minting new shares, raising the value of every existing share.
    ///
    /// Admin-only: requires authorization from the configured admin address.
    pub fn accrue_yield(env: Env, amount: u128) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let admin = storage::get_admin(&env);
        admin.require_auth();

        if amount == 0 {
            return Err(Error::ZeroAmount);
        }

        let total_assets = storage::get_total_assets(&env).saturating_add(amount);
        storage::set_total_assets(&env, total_assets);
        storage::extend_instance(&env);

        events::accrue_yield(&env, amount, total_assets);
        Ok(())
    }

    /// Returns the vault's advertised annual percentage yield, expressed in
    /// basis points (1% == 100 basis points).
    ///
    /// This is a fixed mock figure for demonstration purposes; a production
    /// vault would derive it from observed yield over time.
    pub fn get_apy(_env: Env) -> u32 {
        types::MOCK_APY_BPS
    }

    /// Returns the contract's on-chain interface version.
    pub fn version(_env: Env) -> u32 {
        types::VERSION
    }

    /// Stages the Wasm hash that the next [`Self::upgrade`] call must present.
    ///
    /// Admin-only: requires authorization from the configured admin address.
    /// The hash is stored in instance storage and cleared automatically on a
    /// successful upgrade. Setting a new hash overwrites any previous value,
    /// giving the admin a safe way to correct a staging mistake before applying
    /// the upgrade.
    pub fn set_expected_wasm_hash(env: Env, expected_hash: BytesN<32>) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let admin = storage::get_admin(&env);
        admin.require_auth();

        storage::set_expected_wasm_hash(&env, &expected_hash);
        storage::extend_instance(&env);
        Ok(())
    }

    /// Upgrades the contract's Wasm bytecode to the provided hash.
    ///
    /// Admin-only: requires authorization from the configured admin address.
    ///
    /// The caller must first call [`Self::set_expected_wasm_hash`] to stage the
    /// approved hash. This upgrade call then verifies that `new_wasm_hash`
    /// matches the staged value before invoking the deployer; if the hashes
    /// differ it returns [`Error::WasmHashMismatch`] and the transaction rolls
    /// back without touching the contract's Wasm or any other state.
    ///
    /// On success the staged hash is cleared and an auditable `upgrade` event
    /// is emitted that records both the upgrading admin and the new hash.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let admin = storage::get_admin(&env);
        admin.require_auth();

        // Verify the artifact against the admin-approved expected hash.
        // Any mismatch returns an error and the whole transaction is rolled back
        // atomically — no Wasm swap, no storage mutation, no event.
        match storage::get_expected_wasm_hash(&env) {
            Some(ref expected) if expected == &new_wasm_hash => {}
            _ => return Err(Error::WasmHashMismatch),
        }

        // Hash verified — apply the upgrade, then clean up the staged value.
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        storage::clear_expected_wasm_hash(&env);
        storage::extend_instance(&env);
        events::upgrade(&env, &admin, &new_wasm_hash);
        Ok(())
    }
}
