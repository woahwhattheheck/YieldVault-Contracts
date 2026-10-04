from pathlib import Path

path = Path('src/lib.rs')
source = path.read_text()
old_withdraw = '''    /// Returns the amount of underlying assets `user` could withdraw by
    /// redeeming their entire share balance at the current exchange rate.
    pub fn max_withdraw(env: Env, user: Address) -> Result<u128, Error> {
        let shares = storage::get_balance(&env, &user);
        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        math::convert_to_assets(shares, total_shares, total_assets)
    }
'''
new_withdraw = '''    /// Returns the largest asset amount `user` can redeem in one withdrawal
    /// at the current exchange rate and withdrawal limits.
    ///
    /// The amount is rounded to an actually redeemable whole-share position.
    /// Reading the maximum does not consume or reset the withdrawal budget.
    pub fn max_withdraw(env: Env, user: Address) -> Result<u128, Error> {
        let shares = storage::get_balance(&env, &user);
        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        let shares = Self::withdrawable_shares(&env, shares, total_shares, total_assets)?;
        math::convert_to_assets(shares, total_shares, total_assets)
    }
'''
old_redeem = '''    /// Returns the maximum number of shares `user` can redeem, which is simply
    /// their current share balance.
    ///
    /// Provided as the ERC4626-style counterpart to [`Self::max_withdraw`],
    /// which reports the same position denominated in underlying assets.
    pub fn max_redeem(env: Env, user: Address) -> u128 {
        storage::get_balance(&env, &user)
    }
'''
new_redeem = '''    /// Returns the maximum number of shares `user` can redeem in one
    /// withdrawal under the current per-operation and remaining-period limits.
    ///
    /// This is the share-denominated counterpart to [`Self::max_withdraw`].
    /// It conservatively returns zero if the position cannot be converted by
    /// the contract's checked arithmetic, or would redeem zero assets.
    pub fn max_redeem(env: Env, user: Address) -> u128 {
        let shares = storage::get_balance(&env, &user);
        let total_shares = storage::get_total_shares(&env);
        let total_assets = storage::get_total_assets(&env);
        Self::withdrawable_shares(&env, shares, total_shares, total_assets).unwrap_or(0)
    }
'''
anchor = '''impl YieldVault {
    /// Checks per-operation and rolling-period limits for `op_assets`'''
helper = '''impl YieldVault {
    /// Computes a redeemable position without changing period accounting.
    /// The rollover condition mirrors enforcement, but only a withdrawal
    /// commits the new window. Deposit-only pause does not restrict exits.
    fn withdrawable_shares(
        env: &Env,
        shares: u128,
        total_shares: u128,
        total_assets: u128,
    ) -> Result<u128, Error> {
        let assets = math::convert_to_assets(shares, total_shares, total_assets)?;
        if assets == 0 {
            return Ok(0);
        }
        if storage::is_withdraw_limits_override(env) {
            return Ok(shares);
        }

        let per_op = storage::get_max_withdraw_per_op(env);
        let mut limit = if per_op == 0 { u128::MAX } else { per_op };
        let per_period = storage::get_max_withdraw_per_period(env);
        if per_period > 0 {
            let seconds = storage::get_withdraw_period_secs(env);
            let elapsed = env
                .ledger()
                .timestamp()
                .saturating_sub(storage::get_period_started_at(env));
            let used = if seconds > 0 && elapsed >= seconds {
                0
            } else {
                storage::get_period_withdrawn(env)
            };
            limit = limit.min(per_period.saturating_sub(used));
        }
        if assets <= limit {
            return Ok(shares);
        }
        if limit == 0 {
            return Ok(0);
        }

        // floor(n * total_assets / total_shares) <= limit exactly when
        // n * total_assets < (limit + 1) * total_shares. The strict bound
        // rounds to whole shares without advertising an unattainable amount.
        // Since limit < assets and the full position converted successfully,
        // this intermediate product cannot exceed shares * total_assets.
        let exclusive = limit
            .checked_add(1)
            .and_then(|value| value.checked_mul(total_shares))
            .ok_or(Error::MathOverflow)?;
        Ok(shares.min((exclusive - 1) / total_assets))
    }

    /// Checks per-operation and rolling-period limits for `op_assets`'''
for before, after in [(old_withdraw, new_withdraw), (old_redeem, new_redeem), (anchor, helper)]:
    if source.count(before) != 1:
        raise SystemExit('Expected source block is missing or duplicated; refusing patch')
    source = source.replace(before, after, 1)
path.write_text(source)
