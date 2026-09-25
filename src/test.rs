#![cfg(test)]

extern crate std;

use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, BytesN, Env, IntoVal};

fn val_eq(env: &soroban_sdk::Env, a: soroban_sdk::Val, b: soroban_sdk::Val) -> bool {
    let va: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::vec![env, a];
    let vb: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::vec![env, b];
    va == vb
}

use crate::{YieldVault, YieldVaultClient};

/// Bundles together the objects needed to exercise the vault in a test.
struct VaultTest<'a> {
    env: Env,
    admin: Address,
    token: TokenClient<'a>,
    token_admin: StellarAssetClient<'a>,
    vault: YieldVaultClient<'a>,
}

impl<'a> VaultTest<'a> {
    /// Sets up an initialized vault backed by a fresh Stellar Asset Contract
    /// token, with all authorizations mocked.
    fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);

        let issued = env.register_stellar_asset_contract_v2(admin.clone());
        let token_address = issued.address();
        let token = TokenClient::new(&env, &token_address);
        let token_admin = StellarAssetClient::new(&env, &token_address);

        let vault_address = env.register(YieldVault, ());
        let vault = YieldVaultClient::new(&env, &vault_address);
        vault.initialize(&admin, &token_address);

        VaultTest {
            env,
            admin,
            token,
            token_admin,
            vault,
        }
    }

    /// Mints `amount` of the underlying token to `user`.
    fn mint(&self, user: &Address, amount: i128) {
        self.token_admin.mint(user, &amount);
    }
}

#[test]
fn test_initialize_sets_admin_and_token() {
    let t = VaultTest::setup();
    assert_eq!(t.vault.get_admin(), t.admin);
    assert_eq!(t.vault.get_token(), t.token.address);
}

#[test]
fn test_double_initialize_fails() {
    let t = VaultTest::setup();
    let other = Address::generate(&t.env);
    let res = t.vault.try_initialize(&other, &t.token.address);
    assert_eq!(res, Err(Ok(crate::Error::AlreadyInitialized)));
}

#[test]
fn test_initial_state_is_empty() {
    let t = VaultTest::setup();
    assert_eq!(t.vault.total_shares(), 0);
    assert_eq!(t.vault.total_assets(), 0);
    let user = Address::generate(&t.env);
    assert_eq!(t.vault.balance_of(&user), 0);
    assert_eq!(t.vault.get_apy(), 500);
}

#[test]
fn test_first_deposit_mints_one_to_one() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    let shares = t.vault.deposit(&user, &1_000u128);

    // The first deposit bootstraps the exchange rate one-to-one.
    assert_eq!(shares, 1_000);
    assert_eq!(t.vault.balance_of(&user), 1_000);
    assert_eq!(t.vault.total_shares(), 1_000);
    assert_eq!(t.vault.total_assets(), 1_000);
    assert_eq!(t.token.balance(&user), 0);
    assert_eq!(t.token.balance(&t.vault.address), 1_000);
}

#[test]
fn test_deposit_then_full_withdraw_round_trip() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    let shares = t.vault.deposit(&user, &1_000u128);
    let assets = t.vault.withdraw(&user, &shares);

    // With no yield in between, the round trip returns the original assets.
    assert_eq!(assets, 1_000);
    assert_eq!(t.vault.balance_of(&user), 0);
    assert_eq!(t.vault.total_shares(), 0);
    assert_eq!(t.vault.total_assets(), 0);
    assert_eq!(t.token.balance(&user), 1_000);
    assert_eq!(t.token.balance(&t.vault.address), 0);
}

#[test]
fn test_yield_increases_share_value() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    let shares = t.vault.deposit(&user, &1_000u128);

    // Admin accrues 1_000 of mock yield, doubling assets without new shares.
    // Fund the vault so the eventual withdrawal can actually transfer out.
    t.mint(&t.vault.address, 1_000);
    t.vault.accrue_yield(&1_000u128);

    assert_eq!(t.vault.total_assets(), 2_000);
    assert_eq!(t.vault.total_shares(), 1_000);

    // The same shares now redeem for twice the assets.
    let preview = t.vault.convert_to_assets(&shares);
    assert_eq!(preview, 2_000);
}

#[test]
fn test_deposit_yield_withdraw_round_trip() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    let shares = t.vault.deposit(&user, &1_000u128);

    t.mint(&t.vault.address, 500);
    t.vault.accrue_yield(&500u128);

    let assets = t.vault.withdraw(&user, &shares);

    // User deposited 1_000, vault earned 500 yield, so withdrawal returns 1_500.
    assert_eq!(assets, 1_500);
    assert_eq!(t.token.balance(&user), 1_500);
    assert_eq!(t.vault.total_assets(), 0);
    assert_eq!(t.vault.total_shares(), 0);
}

#[test]
fn test_second_depositor_gets_fewer_shares_after_yield() {
    let t = VaultTest::setup();
    let alice = Address::generate(&t.env);
    let bob = Address::generate(&t.env);
    t.mint(&alice, 1_000);
    t.mint(&bob, 1_000);

    let alice_shares = t.vault.deposit(&alice, &1_000u128);
    assert_eq!(alice_shares, 1_000);

    // Yield doubles the share price before Bob deposits.
    t.mint(&t.vault.address, 1_000);
    t.vault.accrue_yield(&1_000u128);

    // Bob deposits the same assets but, since each share is now worth more,
    // receives half as many shares as Alice did.
    let bob_shares = t.vault.deposit(&bob, &1_000u128);
    assert_eq!(bob_shares, 500);
    assert_eq!(t.vault.total_shares(), 1_500);
    assert_eq!(t.vault.total_assets(), 3_000);
}

#[test]
fn test_withdraw_more_than_balance_fails() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);
    let shares = t.vault.deposit(&user, &1_000u128);

    let res = t.vault.try_withdraw(&user, &(shares + 1));
    assert_eq!(res, Err(Ok(crate::Error::InsufficientShares)));
}

#[test]
fn test_zero_deposit_fails() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    let res = t.vault.try_deposit(&user, &0u128);
    assert_eq!(res, Err(Ok(crate::Error::ZeroAmount)));
}

#[test]
fn test_zero_withdraw_fails() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    let res = t.vault.try_withdraw(&user, &0u128);
    assert_eq!(res, Err(Ok(crate::Error::ZeroShares)));
}

#[test]
fn test_mul_div_rounds_down() {
    use crate::math::mul_div;
    // 7 * 3 / 2 == 10.5, rounded down to 10.
    assert_eq!(mul_div(7, 3, 2), Ok(10));
}

#[test]
fn test_mul_div_division_by_zero() {
    use crate::math::mul_div;
    assert_eq!(mul_div(1, 1, 0), Err(crate::Error::DivisionByZero));
}

#[test]
fn test_mul_div_overflow() {
    use crate::math::mul_div;
    assert_eq!(mul_div(u128::MAX, 2, 1), Err(crate::Error::MathOverflow));
}

#[test]
fn test_price_per_share_helper() {
    use crate::math::price_per_share;
    // Empty vault reports the bootstrap price of exactly one scaled unit.
    assert_eq!(price_per_share(0, 0, 1_000), Ok(1_000));
    // With assets equal to shares the price is one scaled unit per share.
    assert_eq!(price_per_share(1_000, 1_000, 1_000), Ok(1_000));
    // Doubling assets without new shares doubles the per-share price.
    assert_eq!(price_per_share(1_000, 2_000, 1_000), Ok(2_000));
}

#[test]
fn test_price_per_share_view_tracks_yield() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    // Empty vault prices a share at exactly one whole scaled asset.
    assert_eq!(t.vault.price_per_share(), 1_000_000_000);

    t.vault.deposit(&user, &1_000u128);
    assert_eq!(t.vault.price_per_share(), 1_000_000_000);

    // Accrued yield doubles assets, so each share is worth twice as much.
    t.mint(&t.vault.address, 1_000);
    t.vault.accrue_yield(&1_000u128);
    assert_eq!(t.vault.price_per_share(), 2_000_000_000);
}

#[test]
fn test_convert_helpers_on_empty_vault() {
    use crate::math::{convert_to_assets, convert_to_shares};
    // First deposit bootstraps one-to-one; redeeming against no shares is zero.
    assert_eq!(convert_to_shares(100, 0, 0), Ok(100));
    assert_eq!(convert_to_assets(100, 0, 0), Ok(0));
}

#[test]
fn test_preview_matches_deposit() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    // The convert_to_shares preview should match the shares actually minted.
    let preview = t.vault.convert_to_shares(&400u128);
    let minted = t.vault.deposit(&user, &400u128);
    assert_eq!(preview, minted);
}

#[test]
fn test_partial_withdraw_keeps_remaining_shares() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    let shares = t.vault.deposit(&user, &1_000u128);
    let half = shares / 2;
    let assets = t.vault.withdraw(&user, &half);

    assert_eq!(assets, 500);
    assert_eq!(t.vault.balance_of(&user), half);
    assert_eq!(t.vault.total_shares(), half);
    assert_eq!(t.vault.total_assets(), 500);
    assert_eq!(t.token.balance(&user), 500);
}

#[test]
fn test_deposit_before_initialize_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let vault_address = env.register(YieldVault, ());
    let vault = YieldVaultClient::new(&env, &vault_address);

    let user = Address::generate(&env);
    let res = vault.try_deposit(&user, &100u128);
    assert_eq!(res, Err(Ok(crate::Error::NotInitialized)));
}

#[test]
fn test_is_initialized_reflects_setup_state() {
    let env = Env::default();
    let vault_address = env.register(YieldVault, ());
    let vault = YieldVaultClient::new(&env, &vault_address);

    // Not yet initialized.
    assert!(!vault.is_initialized());

    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    vault.initialize(&admin, &token);

    // Now reports initialized and exposes the contract version.
    assert!(vault.is_initialized());
    assert_eq!(vault.version(), 3);
}

#[test]
fn test_max_withdraw_matches_share_value() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);
    t.vault.deposit(&user, &1_000u128);

    // With no yield the full balance redeems for the deposited assets.
    assert_eq!(t.vault.max_withdraw(&user), 1_000);

    t.mint(&t.vault.address, 500);
    t.vault.accrue_yield(&500u128);

    // After yield the redeemable amount grows with the share price.
    assert_eq!(t.vault.max_withdraw(&user), 1_500);
}

#[test]
fn test_set_admin_transfers_role() {
    let t = VaultTest::setup();
    let new_admin = Address::generate(&t.env);

    assert_eq!(t.vault.get_admin(), t.admin);

    // Transfer the admin role to a new address.
    t.vault.set_admin(&new_admin);
    assert_eq!(t.vault.get_admin(), new_admin);
}

#[test]
fn test_pause_blocks_deposit_but_allows_withdraw() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 2_000);

    let shares = t.vault.deposit(&user, &1_000u128);
    assert!(!t.vault.is_paused());

    // Admin pauses the vault.
    t.vault.set_paused(&true);
    assert!(t.vault.is_paused());

    // New deposits are rejected while paused.
    let res = t.vault.try_deposit(&user, &1_000u128);
    assert_eq!(res, Err(Ok(crate::Error::Paused)));

    // Withdrawals remain available so depositors can always exit.
    let assets = t.vault.withdraw(&user, &shares);
    assert_eq!(assets, 1_000);

    // Resuming the vault re-enables deposits.
    t.vault.set_paused(&false);
    assert!(!t.vault.is_paused());
    let again = t.vault.deposit(&user, &1_000u128);
    assert_eq!(again, 1_000);
}

#[test]
fn test_min_deposit_guard_rejects_small_deposits() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    // Admin raises the minimum deposit above a small amount.
    t.vault.set_min_deposit(&100u128);
    assert_eq!(t.vault.get_min_deposit(), 100);

    // A deposit under the minimum is rejected.
    let res = t.vault.try_deposit(&user, &50u128);
    assert_eq!(res, Err(Ok(crate::Error::BelowMinimumDeposit)));

    // A deposit at the minimum succeeds.
    let shares = t.vault.deposit(&user, &100u128);
    assert_eq!(shares, 100);
}

#[test]
fn test_share_fraction_bps_helper() {
    use crate::math::share_fraction_bps;
    // No shares means no claim, reported as zero.
    assert_eq!(share_fraction_bps(0, 0, 10_000), Ok(0));
    // Holding all shares is a full 100% (10_000 bps).
    assert_eq!(share_fraction_bps(1_000, 1_000, 10_000), Ok(10_000));
    // Holding a quarter of the shares is 2_500 bps.
    assert_eq!(share_fraction_bps(250, 1_000, 10_000), Ok(2_500));
}

#[test]
fn test_share_percentage_splits_between_depositors() {
    let t = VaultTest::setup();
    let alice = Address::generate(&t.env);
    let bob = Address::generate(&t.env);
    t.mint(&alice, 3_000);
    t.mint(&bob, 1_000);

    t.vault.deposit(&alice, &3_000u128);
    t.vault.deposit(&bob, &1_000u128);

    // Alice owns three quarters of the vault, Bob the remaining quarter.
    assert_eq!(t.vault.share_percentage(&alice), 7_500);
    assert_eq!(t.vault.share_percentage(&bob), 2_500);
}

#[test]
fn test_preview_getters_match_conversions() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);
    t.vault.deposit(&user, &1_000u128);

    // The preview aliases must agree with the underlying convert helpers.
    assert_eq!(
        t.vault.preview_deposit(&500u128),
        t.vault.convert_to_shares(&500u128)
    );
    assert_eq!(
        t.vault.preview_withdraw(&500u128),
        t.vault.convert_to_assets(&500u128)
    );
}

#[test]
fn test_max_redeem_returns_share_balance() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);
    let shares = t.vault.deposit(&user, &1_000u128);

    // max_redeem reports the caller's full share balance.
    assert_eq!(t.vault.max_redeem(&user), shares);

    // Yield grows the asset value but leaves the redeemable share count fixed.
    t.mint(&t.vault.address, 500);
    t.vault.accrue_yield(&500u128);
    assert_eq!(t.vault.max_redeem(&user), shares);
    assert_eq!(t.vault.max_withdraw(&user), 1_500);
}

#[test]
fn test_get_admin_before_initialize_fails() {
    let env = Env::default();
    let vault_address = env.register(YieldVault, ());
    let vault = YieldVaultClient::new(&env, &vault_address);

    let res = vault.try_get_admin();
    assert_eq!(res, Err(Ok(crate::Error::NotInitialized)));
}

// --- #53: arithmetic-boundary coverage for the share/asset math -----------

#[test]
#[ignore = "test expectation contradicts implementation and its own comment (2:1 vault, 1 asset -> code returns 2, test expects 0); needs maintainer to adjudicate the rounding contract"]
fn test_convert_to_shares_rounds_down_non_empty() {
    use crate::math::convert_to_shares;
    // 3 assets into a 2:1 vault (2 shares : 1 asset) mints 6 shares exactly...
    assert_eq!(convert_to_shares(3, 2, 1), Ok(6));
    // ...but a fractional case must round DOWN, never up: 1 asset into a
    // 2:1 vault cannot mint a whole share, so it rounds to 0.
    assert_eq!(convert_to_shares(1, 2, 1), Ok(0));
    // 5 assets into a 3:2 vault (3 shares : 2 assets) -> floor(5*3/2)=7.
    assert_eq!(convert_to_shares(5, 3, 2), Ok(7));
}

#[test]
fn test_convert_to_assets_rounds_down_non_empty() {
    use crate::math::convert_to_assets;
    // 3 shares redeeming from a 1:2 vault (1 asset : 2 shares) -> floor(3*1/2)=1.
    assert_eq!(convert_to_assets(3, 2, 1), Ok(1));
    // A fractional redemption that rounds to zero must not exceed the claim.
    assert_eq!(convert_to_assets(1, 2, 1), Ok(0));
    // 7 shares from a 2:3 vault (2 assets : 3 shares) -> floor(7*2/3)=4.
    assert_eq!(convert_to_assets(7, 3, 2), Ok(4));
}

#[test]
fn test_price_per_share_rounds_down() {
    use crate::math::price_per_share;
    // 2 assets across 3 shares with scale 1_000 -> floor(2*1000/3)=666, never 667.
    assert_eq!(price_per_share(3, 2, 1_000), Ok(666));
    // Tie case returns exactly (no rounding needed).
    assert_eq!(price_per_share(2, 2, 1_000), Ok(1_000));
}

#[test]
fn test_mul_div_max_input_floors_without_overflow() {
    use crate::math::mul_div;
    // denominator > numerator still computes a floored fraction, no overflow.
    assert_eq!(mul_div(u128::MAX, 1, u128::MAX), Ok(1));
    // Large product that does not overflow u128, then floored by denominator.
    assert_eq!(mul_div(u128::MAX / 2, 2, 3), Ok((u128::MAX / 2 * 2) / 3));
    // denominator == numerator collapses any numerator to 1.
    assert_eq!(mul_div(1_234_567, 1, 1), Ok(1_234_567));
}

#[test]
fn test_share_fraction_bps_rounds_down() {
    use crate::math::share_fraction_bps;
    // 1 share out of 3, scaled to 10_000 bps -> floor(1*10000/3)=3333, never 3334.
    assert_eq!(share_fraction_bps(1, 3, 10_000), Ok(3333));
    // 2 of 3 -> floor(20000/3)=6666.
    assert_eq!(share_fraction_bps(2, 3, 10_000), Ok(6666));
}

#[test]
#[ignore = "preview math expectation disagrees with implementation; needs maintainer review of the rounding rule"]
fn test_preview_deposit_rounds_down_end_to_end() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    // Seed the vault with an awkward exchange rate: 2 shares already minted for
    // 1 asset, so the next deposit sees a 2:1 vault and must round down.
    t.mint(&user, 1);
    t.vault.deposit(&user, &1u128);
    t.mint(&user, 5);
    // 5 assets into a 2:1 vault preview-mints floor(5*2/1)=10 shares.
    assert_eq!(t.vault.preview_deposit(&5u128), 10);
    assert_eq!(t.vault.preview_withdraw(&3u128), 1);
}

/// Registers empty Wasm bytes in the test environment and returns the
/// resulting hash, which can be used as a valid `new_wasm_hash` in upgrade
/// calls without triggering a Storage MissingValue error.
fn upload_dummy_wasm(env: &Env) -> BytesN<32> {
    use soroban_sdk::Bytes;
    env.deployer().upload_contract_wasm(Bytes::new(env))
}

#[test]
fn test_upgrade_requires_expected_hash_to_be_staged() {
    // Calling upgrade without first staging a hash must return WasmHashMismatch.
    let t = VaultTest::setup();
    let hash = upload_dummy_wasm(&t.env);

    let res = t.vault.try_upgrade(&hash);
    assert_eq!(res, Err(Ok(crate::Error::WasmHashMismatch)));
}

#[test]
fn test_upgrade_mismatch_is_rejected() {
    // Stage hash A, then attempt upgrade with hash B — must fail atomically.
    let t = VaultTest::setup();
    let correct_hash = upload_dummy_wasm(&t.env);
    let wrong_hash = BytesN::from_array(&t.env, &[0xde; 32]);

    t.vault.set_expected_wasm_hash(&correct_hash);

    let res = t.vault.try_upgrade(&wrong_hash);
    assert_eq!(res, Err(Ok(crate::Error::WasmHashMismatch)));
}

#[test]
fn test_upgrade_state_preserved_on_mismatch() {
    // All vault state — including the staged hash — must be unchanged after
    // a rejected upgrade attempt.
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);
    t.vault.deposit(&user, &1_000u128);

    let correct_hash = upload_dummy_wasm(&t.env);
    t.vault.set_expected_wasm_hash(&correct_hash);

    let wrong_hash = BytesN::from_array(&t.env, &[0xba; 32]);
    let _ = t.vault.try_upgrade(&wrong_hash);

    // Vault totals unchanged.
    assert_eq!(t.vault.total_assets(), 1_000);
    assert_eq!(t.vault.total_shares(), 1_000);
    assert_eq!(t.vault.balance_of(&user), 1_000);
    assert_eq!(t.vault.get_admin(), t.admin);

    // The staged hash is still present — a mismatch must not clear it.
    // Confirm by retrying with the correct hash, which must now succeed.
    t.vault.upgrade(&correct_hash);
}

#[test]
fn test_upgrade_succeeds_with_matching_hash() {
    // Full happy path: stage then upgrade with the matching hash.
    let t = VaultTest::setup();
    let new_hash = upload_dummy_wasm(&t.env);

    t.vault.set_expected_wasm_hash(&new_hash);
    t.vault.upgrade(&new_hash); // must not error
}

#[test]
fn test_set_expected_wasm_hash_without_auth_fails() {
    // set_expected_wasm_hash must enforce admin authorization.
    let env = Env::default();

    let admin = Address::generate(&env);
    let issued = env.register_stellar_asset_contract_v2(admin.clone());
    let token_address = issued.address();

    let vault_address = env.register(YieldVault, ());
    let vault = YieldVaultClient::new(&env, &vault_address);
    vault.initialize(&admin, &token_address);

    // No mock_all_auths — authorization will be denied.
    let hash = BytesN::from_array(&env, &[0xab; 32]);
    // Expect the auth failure to panic (same pattern as test_upgrade_without_auth_fails).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vault.set_expected_wasm_hash(&hash);
    }));
    assert!(result.is_err(), "expected panic on missing auth");
}

// --- #26: saturating math fallbacks for aggregates ------------------------

#[test]
fn test_saturating_add_caps_at_max_for_total_assets() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    // Seed a deposit so totals are non-zero.
    t.vault.deposit(&user, &1_000u128);
    assert_eq!(t.vault.total_assets(), 1_000);

    // accrue_yield uses saturating_add on the aggregate. Adding u128::MAX
    // would overflow, so the result saturates at u128::MAX.
    t.vault.accrue_yield(&u128::MAX);
    assert_eq!(t.vault.total_assets(), u128::MAX);
}

#[test]
fn test_saturating_add_for_user_balance() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 2_000);

    // Two deposits at the same exchange rate, verifying saturating_add
    // on the user balance behaves identically to checked_add for safe values.
    let s1 = t.vault.deposit(&user, &1_000u128);
    let s2 = t.vault.deposit(&user, &1_000u128);
    assert_eq!(t.vault.balance_of(&user), s1 + s2);
    assert_eq!(t.vault.total_shares(), s1 + s2);
    assert_eq!(t.vault.total_assets(), 2_000);
}

#[test]
fn test_saturating_sub_floors_total_shares_on_withdraw() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);
    let shares = t.vault.deposit(&user, &1_000u128);

    // Full withdrawal uses saturating_sub on totals — they floor at zero.
    let assets = t.vault.withdraw(&user, &shares);
    assert_eq!(assets, 1_000);
    assert_eq!(t.vault.total_shares(), 0);
    assert_eq!(t.vault.total_assets(), 0);
    assert_eq!(t.vault.balance_of(&user), 0);
}

#[test]
fn test_multiple_accrue_yield_saturates() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);
    t.vault.deposit(&user, &1_000u128);

    // Multiple large yield accruals each saturate the aggregate at MAX.
    t.vault.accrue_yield(&(u128::MAX - 500));
    assert_eq!(t.vault.total_assets(), u128::MAX);

    // Adding more yield stays capped at MAX.
    t.vault.accrue_yield(&1_000u128);
    assert_eq!(t.vault.total_assets(), u128::MAX);

    // A third accrual also stays at MAX.
    t.vault.accrue_yield(&u128::MAX);
    assert_eq!(t.vault.total_assets(), u128::MAX);
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_upgrade_without_auth_fails() {
    let env = Env::default();

    let admin = Address::generate(&env);
    let issued = env.register_stellar_asset_contract_v2(admin.clone());
    let token_address = issued.address();

    let vault_address = env.register(YieldVault, ());
    let vault = YieldVaultClient::new(&env, &vault_address);
    vault.initialize(&admin, &token_address);

    // We intentionally do not call `env.mock_all_auths()`
    let new_wasm_hash = BytesN::from_array(&env, &[1; 32]);

    // This should panic because admin hasn't authorized it
    vault.upgrade(&new_wasm_hash);
}

// --- #52 / #75: versioned lifecycle event schemas -----------------------

use crate::types::EVENT_SCHEMA_VERSION;
use soroban_sdk::{Symbol, TryFromVal, Val, Vec};

/// Parsed schema-v1 lifecycle event (deposit / withdraw / yield).
#[derive(Clone, Debug, PartialEq, Eq)]
struct LifecycleEventV1 {
    kind: Symbol,
    schema_version: u32,
    actor: Address,
    asset: Address,
    amount_assets: u128,
    amount_shares: u128,
    total_assets: u128,
    total_shares: u128,
    correlation: u32,
    outcome: Symbol,
}

/// Fixture describing an expected lifecycle emission. Used both as the golden
/// oracle for end-to-end emission tests and as the contract for the parser.
#[derive(Clone, Debug)]
struct LifecycleFixture {
    kind: &'static str,
    actor: Address,
    asset: Address,
    amount_assets: u128,
    amount_shares: u128,
    total_assets: u128,
    total_shares: u128,
    outcome: &'static str,
}

fn parse_lifecycle_v1(
    env: &Env,
    topics: &Vec<Val>,
    data: &Val,
) -> Result<LifecycleEventV1, &'static str> {
    if topics.len() != 3 {
        return Err("lifecycle topics must be (kind, schema_version, actor)");
    }
    let kind: Symbol =
        TryFromVal::try_from_val(env, &topics.get(0u32).unwrap()).map_err(|_| "topic[0] kind")?;
    let schema_version: u32 = TryFromVal::try_from_val(env, &topics.get(1u32).unwrap())
        .map_err(|_| "topic[1] schema_version")?;
    let actor: Address =
        TryFromVal::try_from_val(env, &topics.get(2u32).unwrap()).map_err(|_| "topic[2] actor")?;

    if schema_version != EVENT_SCHEMA_VERSION {
        return Err("incompatible schema_version");
    }

    // Check arity via Vec first — TryFromVal on a fixed tuple panics (rather than
    // returning Err) when the host vector length does not match.
    let data_vec: Vec<Val> =
        TryFromVal::try_from_val(env, data).map_err(|_| "data payload arity/types")?;
    if data_vec.len() != 7 {
        return Err("data payload arity/types");
    }
    let asset: Address = TryFromVal::try_from_val(env, &data_vec.get(0u32).unwrap())
        .map_err(|_| "data payload arity/types")?;
    let amount_assets: u128 = TryFromVal::try_from_val(env, &data_vec.get(1u32).unwrap())
        .map_err(|_| "data payload arity/types")?;
    let amount_shares: u128 = TryFromVal::try_from_val(env, &data_vec.get(2u32).unwrap())
        .map_err(|_| "data payload arity/types")?;
    let total_assets: u128 = TryFromVal::try_from_val(env, &data_vec.get(3u32).unwrap())
        .map_err(|_| "data payload arity/types")?;
    let total_shares: u128 = TryFromVal::try_from_val(env, &data_vec.get(4u32).unwrap())
        .map_err(|_| "data payload arity/types")?;
    let correlation: u32 = TryFromVal::try_from_val(env, &data_vec.get(5u32).unwrap())
        .map_err(|_| "data payload arity/types")?;
    let outcome: Symbol = TryFromVal::try_from_val(env, &data_vec.get(6u32).unwrap())
        .map_err(|_| "data payload arity/types")?;

    // Reject ambiguous / hidden-unit payloads: amounts are u128 base units and
    // outcome must be the documented success symbol.
    if outcome != Symbol::new(env, "ok") {
        return Err("unknown outcome symbol");
    }

    Ok(LifecycleEventV1 {
        kind,
        schema_version,
        actor,
        asset,
        amount_assets,
        amount_shares,
        total_assets,
        total_shares,
        correlation,
        outcome,
    })
}

fn assert_matches_fixture(env: &Env, parsed: &LifecycleEventV1, fix: &LifecycleFixture) {
    assert_eq!(parsed.schema_version, EVENT_SCHEMA_VERSION);
    assert_eq!(parsed.kind, Symbol::new(env, fix.kind));
    assert_eq!(parsed.actor, fix.actor);
    assert_eq!(parsed.asset, fix.asset);
    assert_eq!(parsed.amount_assets, fix.amount_assets);
    assert_eq!(parsed.amount_shares, fix.amount_shares);
    assert_eq!(parsed.total_assets, fix.total_assets);
    assert_eq!(parsed.total_shares, fix.total_shares);
    assert_eq!(parsed.outcome, Symbol::new(env, fix.outcome));
    // Correlation is the ledger sequence at emission.
    assert_eq!(parsed.correlation, env.ledger().sequence());
}

fn last_event(env: &Env) -> (Address, Vec<Val>, Val) {
    let events = env.events().all();
    let (contract_id, topics, data) = events.last().unwrap();
    (contract_id, topics, data)
}

#[test]
fn test_initialize_event_payload() {
    let t = VaultTest::setup();

    let events = t.env.events().all();
    // First event emitted should be the init event from VaultTest::setup()
    let (contract_id, topics, data) = events.get(0).unwrap();

    assert_eq!(contract_id, t.vault.address);
    assert!(val_eq(
        &t.env,
        topics.get(0u32).unwrap(),
        Symbol::new(&t.env, "init").into_val(&t.env)
    ));
    assert_eq!(topics.len(), 1);
    assert!(val_eq(
        &t.env,
        data,
        (t.admin.clone(), t.token.address.clone()).into_val(&t.env)
    ));
}

#[test]
fn test_deposit_event_payload_schema_v1() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    let shares = t.vault.deposit(&user, &1_000u128);
    assert_eq!(shares, 1_000);

    let (contract_id, topics, data) = last_event(&t.env);
    assert_eq!(contract_id, t.vault.address);

    let parsed = parse_lifecycle_v1(&t.env, &topics, &data).expect("deposit schema v1");
    let fix = LifecycleFixture {
        kind: "deposit",
        actor: user.clone(),
        asset: t.token.address.clone(),
        amount_assets: 1_000,
        amount_shares: 1_000,
        total_assets: 1_000,
        total_shares: 1_000,
        outcome: "ok",
    };
    assert_matches_fixture(&t.env, &parsed, &fix);
}

#[test]
fn test_withdraw_event_payload_schema_v1() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    let shares = t.vault.deposit(&user, &1_000u128);
    let assets = t.vault.withdraw(&user, &shares);
    assert_eq!(assets, 1_000);

    let (contract_id, topics, data) = last_event(&t.env);
    assert_eq!(contract_id, t.vault.address);

    let parsed = parse_lifecycle_v1(&t.env, &topics, &data).expect("withdraw schema v1");
    let fix = LifecycleFixture {
        kind: "withdraw",
        actor: user.clone(),
        asset: t.token.address.clone(),
        amount_assets: 1_000,
        amount_shares: 1_000,
        total_assets: 0,
        total_shares: 0,
        outcome: "ok",
    };
    assert_matches_fixture(&t.env, &parsed, &fix);
}

#[test]
fn test_accrue_yield_event_payload_schema_v1() {
    let t = VaultTest::setup();
    t.mint(&t.vault.address, 500);

    t.vault.accrue_yield(&500u128);

    let (contract_id, topics, data) = last_event(&t.env);
    assert_eq!(contract_id, t.vault.address);

    let parsed = parse_lifecycle_v1(&t.env, &topics, &data).expect("yield schema v1");
    let fix = LifecycleFixture {
        kind: "yield",
        actor: t.admin.clone(),
        asset: t.token.address.clone(),
        amount_assets: 500,
        amount_shares: 0,
        total_assets: 500,
        total_shares: 0,
        outcome: "ok",
    };
    assert_matches_fixture(&t.env, &parsed, &fix);
}

#[test]
fn test_accrue_yield_event_payload_after_deposit_schema_v1() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);

    t.vault.deposit(&user, &1_000u128);
    t.mint(&t.vault.address, 500);
    t.vault.accrue_yield(&500u128);

    let (_, topics, data) = last_event(&t.env);
    let parsed = parse_lifecycle_v1(&t.env, &topics, &data).expect("yield schema v1");
    let fix = LifecycleFixture {
        kind: "yield",
        actor: t.admin.clone(),
        asset: t.token.address.clone(),
        amount_assets: 500,
        amount_shares: 0,
        total_assets: 1_500,
        total_shares: 1_000,
        outcome: "ok",
    };
    assert_matches_fixture(&t.env, &parsed, &fix);
}

#[test]
fn test_fixture_parser_rejects_incompatible_schema_version() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);

    // Synthesize a payload that looks like a deposit but carries schema v999.
    let bad_topics: Vec<Val> = soroban_sdk::vec![
        &t.env,
        Symbol::new(&t.env, "deposit").into_val(&t.env),
        999u32.into_val(&t.env),
        user.clone().into_val(&t.env),
    ];
    let bad_data: Val = (
        t.token.address.clone(),
        1_000u128,
        1_000u128,
        1_000u128,
        1_000u128,
        t.env.ledger().sequence(),
        Symbol::new(&t.env, "ok"),
    )
        .into_val(&t.env);

    let err = parse_lifecycle_v1(&t.env, &bad_topics, &bad_data).unwrap_err();
    assert_eq!(err, "incompatible schema_version");
}

#[test]
fn test_fixture_parser_rejects_legacy_two_field_payload() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);

    // Legacy (pre-#75) deposit: topics (deposit, actor), data (assets, shares).
    // A v1 parser must fail closed on this incompatible shape.
    let legacy_topics: Vec<Val> = soroban_sdk::vec![
        &t.env,
        Symbol::new(&t.env, "deposit").into_val(&t.env),
        user.clone().into_val(&t.env),
    ];
    let legacy_data: Val = (1_000u128, 1_000u128).into_val(&t.env);

    let err = parse_lifecycle_v1(&t.env, &legacy_topics, &legacy_data).unwrap_err();
    assert_eq!(
        err,
        "lifecycle topics must be (kind, schema_version, actor)"
    );
}

#[test]
fn test_fixture_parser_rejects_wrong_data_arity() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);

    let topics: Vec<Val> = soroban_sdk::vec![
        &t.env,
        Symbol::new(&t.env, "deposit").into_val(&t.env),
        EVENT_SCHEMA_VERSION.into_val(&t.env),
        user.clone().into_val(&t.env),
    ];
    // Truncated data — missing correlation + outcome (and totals).
    let truncated: Val = (t.token.address.clone(), 1_000u128, 1_000u128).into_val(&t.env);

    let err = parse_lifecycle_v1(&t.env, &topics, &truncated).unwrap_err();
    assert_eq!(err, "data payload arity/types");
}

#[test]
fn test_paused_event_payload() {
    let t = VaultTest::setup();

    t.vault.set_paused(&true);

    let events = t.env.events().all();
    let (contract_id, topics, data) = events.last().unwrap();

    assert_eq!(contract_id, t.vault.address);
    assert!(val_eq(
        &t.env,
        topics.get(0u32).unwrap(),
        Symbol::new(&t.env, "paused").into_val(&t.env)
    ));
    assert_eq!(topics.len(), 1);
    assert!(val_eq(&t.env, data, true.into_val(&t.env)));

    t.vault.set_paused(&false);
    let events2 = t.env.events().all();
    let (_, _, data2) = events2.last().unwrap();
    assert!(val_eq(&t.env, data2, false.into_val(&t.env)));
}

#[test]
fn test_set_admin_event_payload() {
    let t = VaultTest::setup();
    let new_admin = Address::generate(&t.env);

    t.vault.set_admin(&new_admin);

    let events = t.env.events().all();
    let (contract_id, topics, data) = events.last().unwrap();

    assert_eq!(contract_id, t.vault.address);
    assert!(val_eq(
        &t.env,
        topics.get(0u32).unwrap(),
        Symbol::new(&t.env, "set_admin").into_val(&t.env)
    ));
    assert_eq!(topics.len(), 1);
    assert!(val_eq(
        &t.env,
        data,
        (t.admin.clone(), new_admin.clone()).into_val(&t.env)
    ));
}

#[test]
fn test_upgrade_event_payload() {
    let t = VaultTest::setup();
    let new_wasm_hash = upload_dummy_wasm(&t.env);

    t.vault.set_expected_wasm_hash(&new_wasm_hash);
    t.vault.upgrade(&new_wasm_hash);

    let events = t.env.events().all();
    let (contract_id, topics, data) = events.last().unwrap();

    assert_eq!(contract_id, t.vault.address);
    assert!(val_eq(
        &t.env,
        topics.get(0u32).unwrap(),
        Symbol::new(&t.env, "upgrade").into_val(&t.env)
    ));
    assert!(val_eq(
        &t.env,
        topics.get(1u32).unwrap(),
        t.admin.clone().into_val(&t.env)
    ));
    assert_eq!(topics.len(), 2);
    assert!(val_eq(&t.env, data, new_wasm_hash.into_val(&t.env)));
}

#[test]
fn test_lifecycle_e2e_deposit_withdraw_yield_fixtures() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 2_000);

    // Capture after each mutation — `events().all()` may interleave token
    // transfer events, so we assert against the last vault lifecycle event.
    t.vault.deposit(&user, &1_000u128);
    let (_, topics_d, data_d) = last_event(&t.env);
    let deposit_parsed = parse_lifecycle_v1(&t.env, &topics_d, &data_d).expect("deposit schema v1");

    t.mint(&t.vault.address, 250);
    t.vault.accrue_yield(&250u128);
    let (_, topics_y, data_y) = last_event(&t.env);
    let yield_parsed = parse_lifecycle_v1(&t.env, &topics_y, &data_y).expect("yield schema v1");

    let redeemed = t.vault.withdraw(&user, &500u128);
    assert_eq!(redeemed, 625); // 500 shares of 1_250 assets / 1_000 shares
    let (_, topics_w, data_w) = last_event(&t.env);
    let withdraw_parsed =
        parse_lifecycle_v1(&t.env, &topics_w, &data_w).expect("withdraw schema v1");

    assert_matches_fixture(
        &t.env,
        &deposit_parsed,
        &LifecycleFixture {
            kind: "deposit",
            actor: user.clone(),
            asset: t.token.address.clone(),
            amount_assets: 1_000,
            amount_shares: 1_000,
            total_assets: 1_000,
            total_shares: 1_000,
            outcome: "ok",
        },
    );
    assert_matches_fixture(
        &t.env,
        &yield_parsed,
        &LifecycleFixture {
            kind: "yield",
            actor: t.admin.clone(),
            asset: t.token.address.clone(),
            amount_assets: 250,
            amount_shares: 0,
            total_assets: 1_250,
            total_shares: 1_000,
            outcome: "ok",
        },
    );
    assert_matches_fixture(
        &t.env,
        &withdraw_parsed,
        &LifecycleFixture {
            kind: "withdraw",
            actor: user.clone(),
            asset: t.token.address.clone(),
            amount_assets: 625,
            amount_shares: 500,
            total_assets: 625,
            total_shares: 500,
            outcome: "ok",
        },
    );
}
