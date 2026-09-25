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

use soroban_sdk::{contract, contractimpl, contractmeta, token, Address, BytesN, Env, Vec};

contractmeta!(
    key = "Description",
    val = "Share-based ERC4626-style yield vault"
);

/// The YieldVault contract type.
#[contract]
pub struct YieldVault;

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
        // Withdrawal limits default to unlimited (`0`) with a 1-day rolling
        // window ready for operators to tighten without a storage migration.
        storage::set_max_withdraw_per_op(&env, 0);
        storage::set_max_withdraw_per_period(&env, 0);
        storage::set_withdraw_period_secs(&env, types::DEFAULT_WITHDRAW_PERIOD_SECS);
        storage::set_period_withdrawn(&env, 0);
        storage::set_period_started_at(&env, env.ledger().timestamp());
        storage::set_withdraw_limits_override(&env, false);
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
    /// Requires authorization from `from`. The underlying tokens are pulled
    /// from `from` into the vault via the token contract's `transfer`.
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

        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        let shares = math::convert_to_shares(amount, total_shares, total_assets)?;
        if shares == 0 {
            return Err(Error::ZeroShares);
        }

        let token_address = storage::get_token(&env);
        let client = token::Client::new(&env, &token_address);
        client.transfer(&from, &env.current_contract_address(), &(amount as i128));

        let new_total_shares = total_shares.saturating_add(shares);
        let new_total_assets = total_assets.saturating_add(amount);
        let user_balance = storage::get_balance(&env, &from).saturating_add(shares);

        storage::set_total_shares(&env, new_total_shares);
        storage::set_total_assets(&env, new_total_assets);
        storage::set_balance(&env, &from, user_balance);
        storage::extend_instance(&env);

        events::deposit(&env, &from, amount, shares);
        Ok(shares)
    }

    /// Burns `shares` from `from` and returns the corresponding amount of
    /// underlying assets, transferring them back to `from`.
    ///
    /// Requires authorization from `from`. Returns [`Error::InsufficientShares`]
    /// if `from` does not hold enough shares. Subject to the configured
    /// per-operation and rolling-period withdrawal limits (units: underlying
    /// assets) unless the admin override is enabled.
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

        // Enforce limits before any state mutation so over-limit attempts fail
        // atomically (host rolls back the whole invocation).
        Self::enforce_withdraw_limits(&env, assets, assets)?;

        Self::apply_withdraw(
            &env,
            &from,
            shares,
            assets,
            user_balance,
            total_shares,
            total_assets,
        );
        Ok(assets)
    }

    /// Redeems multiple share amounts for `from` in one invocation.
    ///
    /// Each leg is checked against the per-operation asset cap; the *sum* of
    /// redeemed assets is checked against the rolling-period aggregate so a
    /// batch cannot bypass the period limit by splitting. On any failure the
    /// whole batch reverts with no partial burns or transfers.
    ///
    /// Asset amounts are computed at the pre-batch exchange rate (same as a
    /// sequence of single withdraws before any share-price change).
    pub fn withdraw_batch(env: Env, from: Address, shares_list: Vec<u128>) -> Result<u128, Error> {
        storage::require_initialized(&env)?;
        from.require_auth();

        if shares_list.is_empty() {
            return Err(Error::EmptyBatch);
        }

        let user_balance = storage::get_balance(&env, &from);
        let total_shares = storage::get_total_shares(&env);
        let total_assets_vault = storage::get_total_assets(&env);

        let mut legs: Vec<(u128, u128)> = Vec::new(&env);
        let mut batch_assets: u128 = 0;
        let mut shares_needed: u128 = 0;
        let mut i = 0u32;
        while i < shares_list.len() {
            let shares = shares_list.get(i).unwrap();
            if shares == 0 {
                return Err(Error::ZeroShares);
            }
            shares_needed = shares_needed.saturating_add(shares);
            let assets = math::convert_to_assets(shares, total_shares, total_assets_vault)?;
            if assets == 0 {
                return Err(Error::ZeroAmount);
            }
            // Per-operation limit applies to each leg individually.
            let max_per_op = storage::get_max_withdraw_per_op(&env);
            if !storage::is_withdraw_limits_override(&env) && max_per_op > 0 && assets > max_per_op
            {
                return Err(Error::WithdrawLimitExceeded);
            }
            batch_assets = batch_assets.saturating_add(assets);
            legs.push_back((shares, assets));
            i += 1;
        }

        if user_balance < shares_needed {
            return Err(Error::InsufficientShares);
        }

        // Period aggregate sees the full batch sum — splitting cannot bypass it.
        // Pass `op_assets = 0` here because per-op was already checked per leg.
        Self::enforce_withdraw_limits(&env, 0, batch_assets)?;

        let mut remaining_user = user_balance;
        let mut rem_shares = total_shares;
        let mut rem_assets = total_assets_vault;
        let mut m = 0u32;
        while m < legs.len() {
            let (shares, assets) = legs.get(m).unwrap();
            Self::apply_withdraw(
                &env,
                &from,
                shares,
                assets,
                remaining_user,
                rem_shares,
                rem_assets,
            );
            remaining_user = remaining_user.saturating_sub(shares);
            rem_shares = rem_shares.saturating_sub(shares);
            rem_assets = rem_assets.saturating_sub(assets);
            m += 1;
        }

        Ok(batch_assets)
    }

    /// Configures per-operation and rolling-period withdrawal limits.
    ///
    /// Units are underlying assets. `0` for either cap means unlimited.
    /// `period_secs` must be non-zero when `max_per_period > 0`. Admin-only.
    /// Emits a `wd_limits` event. Does not reset period usage; call
    /// [`Self::reset_withdraw_period`] for an auditable usage clear.
    pub fn set_withdraw_limits(
        env: Env,
        max_per_op: u128,
        max_per_period: u128,
        period_secs: u64,
    ) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let admin = storage::get_admin(&env);
        admin.require_auth();

        if max_per_period > 0 && period_secs == 0 {
            return Err(Error::InvalidWithdrawLimit);
        }

        storage::set_max_withdraw_per_op(&env, max_per_op);
        storage::set_max_withdraw_per_period(&env, max_per_period);
        storage::set_withdraw_period_secs(&env, period_secs);
        storage::extend_instance(&env);
        events::withdraw_limits(&env, max_per_op, max_per_period, period_secs);
        Ok(())
    }

    /// Clears rolling-period withdrawal usage and starts a fresh window at the
    /// current ledger timestamp. Admin-only. Emits `wd_reset`.
    pub fn reset_withdraw_period(env: Env) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let admin = storage::get_admin(&env);
        admin.require_auth();

        let cleared = storage::get_period_withdrawn(&env);
        let at = env.ledger().timestamp();
        storage::set_period_withdrawn(&env, 0);
        storage::set_period_started_at(&env, at);
        storage::extend_instance(&env);
        events::withdraw_period_reset(&env, cleared, at);
        Ok(())
    }

    /// Enables or disables the emergency withdrawal-limit override.
    ///
    /// When enabled, per-operation and rolling-period checks are skipped so
    /// operators can unblock legitimate exits. Admin-only. Emits `wd_override`.
    pub fn set_withdraw_limits_override(env: Env, enabled: bool) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let admin = storage::get_admin(&env);
        admin.require_auth();

        storage::set_withdraw_limits_override(&env, enabled);
        storage::extend_instance(&env);
        events::withdraw_override(&env, enabled);
        Ok(())
    }

    /// Returns the per-operation withdrawal asset cap (`0` = unlimited).
    pub fn get_max_withdraw_per_op(env: Env) -> u128 {
        storage::get_max_withdraw_per_op(&env)
    }

    /// Returns the rolling-period withdrawal asset cap (`0` = unlimited).
    pub fn get_max_withdraw_per_period(env: Env) -> u128 {
        storage::get_max_withdraw_per_period(&env)
    }

    /// Returns the rolling withdrawal window length in seconds.
    pub fn get_withdraw_period_secs(env: Env) -> u64 {
        storage::get_withdraw_period_secs(&env)
    }

    /// Returns assets withdrawn so far in the current rolling period.
    pub fn get_period_withdrawn(env: Env) -> u128 {
        storage::get_period_withdrawn(&env)
    }

    /// Returns the ledger timestamp when the current rolling period started.
    pub fn get_period_started_at(env: Env) -> u64 {
        storage::get_period_started_at(&env)
    }

    /// Returns `true` when the emergency withdrawal-limit override is on.
    pub fn is_withdraw_limits_override(env: Env) -> bool {
        storage::is_withdraw_limits_override(&env)
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

impl YieldVault {
    /// Checks per-operation and rolling-period limits for `op_assets` (the
    /// amount counting against the per-op cap) and `period_assets` (the amount
    /// added to the rolling aggregate). Records period usage on success.
    ///
    /// When the emergency override is enabled, checks are skipped and usage is
    /// not recorded so operators can drain/exit without polluting the window.
    fn enforce_withdraw_limits(
        env: &Env,
        op_assets: u128,
        period_assets: u128,
    ) -> Result<(), Error> {
        if storage::is_withdraw_limits_override(env) {
            return Ok(());
        }

        let max_per_op = storage::get_max_withdraw_per_op(env);
        if max_per_op > 0 && op_assets > max_per_op {
            return Err(Error::WithdrawLimitExceeded);
        }

        let max_per_period = storage::get_max_withdraw_per_period(env);
        if max_per_period == 0 {
            return Ok(());
        }

        let period_secs = storage::get_withdraw_period_secs(env);
        let now = env.ledger().timestamp();
        let started = storage::get_period_started_at(env);
        let mut withdrawn = storage::get_period_withdrawn(env);

        // Roll the window forward when the configured period has elapsed.
        if period_secs > 0 && now.saturating_sub(started) >= period_secs {
            withdrawn = 0;
            storage::set_period_started_at(env, now);
            storage::set_period_withdrawn(env, 0);
        }

        let new_withdrawn = withdrawn.saturating_add(period_assets);
        if new_withdrawn > max_per_period {
            return Err(Error::WithdrawPeriodLimitExceeded);
        }
        storage::set_period_withdrawn(env, new_withdrawn);
        Ok(())
    }

    /// Applies share burn, aggregate updates, token push, and withdraw event
    /// for a single redemption leg. Caller must have already enforced limits
    /// and validated balances.
    fn apply_withdraw(
        env: &Env,
        from: &Address,
        shares: u128,
        assets: u128,
        user_balance: u128,
        total_shares: u128,
        total_assets: u128,
    ) {
        let new_total_shares = total_shares.saturating_sub(shares);
        let new_total_assets = total_assets.saturating_sub(assets);
        let new_user_balance = user_balance.saturating_sub(shares);

        storage::set_total_shares(env, new_total_shares);
        storage::set_total_assets(env, new_total_assets);
        storage::set_balance(env, from, new_user_balance);

        let token_address = storage::get_token(env);
        let client = token::Client::new(env, &token_address);
        client.transfer(&env.current_contract_address(), from, &(assets as i128));

        storage::extend_instance(env);
        events::withdraw(env, from, shares, assets);
    }
}
