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

    /// Proposes transferring the admin role to `new_admin`.
    ///
    /// Two-step rotation: the current admin proposes, then `new_admin` must
    /// call [`Self::accept_admin`] before [`types::ADMIN_PROPOSAL_TTL_SECS`]
    /// elapses. The current admin can cancel with
    /// [`Self::cancel_admin_proposal`]. A new proposal overwrites any prior
    /// unaccepted (or expired) proposal.
    ///
    /// Admin-only: requires authorization from the current admin. Emits an
    /// `admin_proposed` event with the expiry timestamp.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let current = storage::get_admin(&env);
        current.require_auth();

        if new_admin == current {
            return Err(Error::InvalidAdminProposal);
        }

        let now = env.ledger().timestamp();
        let expires_at = now.saturating_add(types::ADMIN_PROPOSAL_TTL_SECS);
        storage::set_pending_admin(&env, &new_admin);
        storage::set_admin_proposal_expiry(&env, expires_at);
        storage::extend_instance(&env);
        events::admin_proposed(&env, &current, &new_admin, expires_at);
        Ok(())
    }

    /// Accepts a pending admin-rotation proposal, transferring the admin role
    /// to the caller.
    ///
    /// Requires authorization from the pending administrator. Rejects expired
    /// proposals without changing the active admin. Stale proposals remain
    /// staged until cancelled or overwritten by a new propose.
    pub fn accept_admin(env: Env) -> Result<(), Error> {
        storage::require_initialized(&env)?;

        let pending = storage::get_pending_admin(&env).ok_or(Error::NoPendingAdminProposal)?;
        pending.require_auth();

        let expires_at = storage::get_admin_proposal_expiry(&env)
            .ok_or(Error::NoPendingAdminProposal)?;
        let now = env.ledger().timestamp();
        if now > expires_at {
            // Do not mutate storage here: returning Err rolls the invocation
            // back, so a clear would not persist. The stale proposal remains
            // until the current admin cancels or proposes again.
            return Err(Error::AdminProposalExpired);
        }

        let previous = storage::get_admin(&env);
        storage::set_admin(&env, &pending);
        storage::clear_admin_proposal(&env);
        storage::extend_instance(&env);
        events::admin_accepted(&env, &previous, &pending);
        Ok(())
    }

    /// Cancels an unaccepted admin-rotation proposal.
    ///
    /// Admin-only: requires authorization from the current (still-active)
    /// administrator. Leaves the active admin unchanged.
    pub fn cancel_admin_proposal(env: Env) -> Result<(), Error> {
        storage::require_initialized(&env)?;
        let current = storage::get_admin(&env);
        current.require_auth();

        let pending = storage::get_pending_admin(&env).ok_or(Error::NoPendingAdminProposal)?;
        storage::clear_admin_proposal(&env);
        storage::extend_instance(&env);
        events::admin_proposal_cancelled(&env, &current, &pending);
        Ok(())
    }

    /// Returns the pending administrator address, if a proposal is staged.
    ///
    /// Does not check expiry; callers should also read
    /// [`Self::get_admin_proposal_expiry`] when they need to know whether the
    /// proposal is still acceptable.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        storage::get_pending_admin(&env)
    }

    /// Returns the unix timestamp when the pending admin proposal expires, if
    /// one is staged.
    pub fn get_admin_proposal_expiry(env: Env) -> Option<u64> {
        storage::get_admin_proposal_expiry(&env)
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
    /// if `from` does not hold enough shares.
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

        let new_total_shares = total_shares.saturating_sub(shares);
        let new_total_assets = total_assets.saturating_sub(assets);
        let new_user_balance = user_balance.saturating_sub(shares);

        storage::set_total_shares(&env, new_total_shares);
        storage::set_total_assets(&env, new_total_assets);
        storage::set_balance(&env, &from, new_user_balance);

        let token_address = storage::get_token(&env);
        let client = token::Client::new(&env, &token_address);
        client.transfer(&env.current_contract_address(), &from, &(assets as i128));

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
