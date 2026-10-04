
#[test]
fn test_max_withdraw_limit_active_caps_and_zero_budget() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    t.mint(&user, 1_000);
    t.vault.deposit(&user, &1_000);
    t.vault.set_withdraw_limits(&400, &700, &60);

    assert_eq!(t.vault.max_withdraw(&user), 400);
    assert_eq!(t.vault.max_redeem(&user), 400);
    assert_eq!(t.vault.withdraw(&user, &400), 400);
    assert_eq!(t.vault.max_withdraw(&user), 300);
    assert_eq!(t.vault.max_redeem(&user), 300);
    assert_eq!(t.vault.get_period_withdrawn(), 400);
    assert_eq!(t.vault.withdraw(&user, &300), 300);
    assert_eq!(t.vault.max_withdraw(&user), 0);
    assert_eq!(t.vault.max_redeem(&user), 0);
    assert_eq!(t.vault.balance_of(&user), 300);

    t.vault.set_withdraw_limits(&400, &600, &60);
    assert_eq!(t.vault.max_withdraw(&user), 0);
    assert_eq!(t.vault.max_redeem(&user), 0);
    t.vault.reset_withdraw_period();
    assert_eq!(t.vault.max_withdraw(&user), 300);
    assert_eq!(t.vault.max_redeem(&user), 300);
}

#[test]
fn test_max_withdraw_limit_shared_period_and_rollover() {
    let t = VaultTest::setup();
    t.env.ledger().with_mut(|info| info.timestamp = 100);
    t.vault.reset_withdraw_period();
    let alice = Address::generate(&t.env);
    let bob = Address::generate(&t.env);
    for user in [&alice, &bob] {
        t.mint(user, 1_000);
        t.vault.deposit(user, &1_000);
    }
    t.vault.set_withdraw_limits(&0, &500, &60);
    t.vault.withdraw(&bob, &400);
    assert_eq!(t.vault.max_withdraw(&alice), 100);
    assert_eq!(t.vault.max_redeem(&alice), 100);
    assert_eq!(t.vault.get_period_started_at(), 100);
    assert_eq!(t.vault.get_period_withdrawn(), 400);

    t.env.ledger().with_mut(|info| info.timestamp = 159);
    assert_eq!(t.vault.max_withdraw(&alice), 100);
    t.env.ledger().with_mut(|info| info.timestamp = 160);
    assert_eq!(t.vault.max_withdraw(&alice), 500);
    assert_eq!(t.vault.max_redeem(&alice), 500);
    assert_eq!(t.vault.max_withdraw(&alice), 500);
    assert_eq!(t.vault.get_period_started_at(), 100);
    assert_eq!(t.vault.get_period_withdrawn(), 400);
    assert_eq!(t.vault.balance_of(&alice), 1_000);
    assert_eq!(t.vault.withdraw(&alice, &500), 500);
    assert_eq!(t.vault.get_period_started_at(), 160);
    assert_eq!(t.vault.get_period_withdrawn(), 500);
    assert_eq!(t.vault.max_withdraw(&alice), 0);
    assert_eq!(t.vault.max_redeem(&bob), 0);
}

#[test]
fn test_max_withdraw_limit_rounds_to_redeemable_shares() {
    let t = VaultTest::setup();
    t.vault.set_min_deposit(&1);
    let user = Address::generate(&t.env);
    t.mint(&user, 2);
    t.vault.deposit(&user, &2);
    t.mint(&t.vault.address, 1);
    t.vault.accrue_yield(&1);
    t.vault.set_withdraw_limits(&2, &0, &60);

    // At a 3/2 asset/share ratio, 2 assets cannot be redeemed exactly.
    assert_eq!(t.vault.max_redeem(&user), 1);
    assert_eq!(t.vault.max_withdraw(&user), 1);
    assert_eq!(
        t.vault.try_withdraw(&user, &2),
        Err(Ok(crate::Error::WithdrawLimitExceeded))
    );
    assert_eq!(t.vault.withdraw(&user, &t.vault.max_redeem(&user)), 1);
    assert_eq!(t.vault.max_redeem(&user), 1);
    assert_eq!(t.vault.max_withdraw(&user), 2);
    assert_eq!(t.vault.withdraw(&user, &1), 2);
    assert_eq!(t.vault.max_withdraw(&user), 0);
    assert_eq!(t.vault.max_redeem(&user), 0);
}

#[test]
fn test_max_withdraw_limit_defaults_pause_and_override() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    assert_eq!(t.vault.max_withdraw(&user), 0);
    assert_eq!(t.vault.max_redeem(&user), 0);
    t.mint(&user, 1_000);
    t.vault.deposit(&user, &1_000);
    assert_eq!(t.vault.max_withdraw(&user), 1_000);
    assert_eq!(t.vault.max_redeem(&user), 1_000);
    t.vault.set_paused(&true);
    assert_eq!(t.vault.max_withdraw(&user), 1_000);
    assert_eq!(t.vault.max_redeem(&user), 1_000);

    t.vault.set_withdraw_limits(&400, &0, &60);
    assert_eq!(t.vault.max_withdraw(&user), 400);
    assert_eq!(t.vault.max_redeem(&user), 400);
    t.vault.set_withdraw_limits(&0, &0, &60);
    assert_eq!(t.vault.max_withdraw(&user), 1_000);
    t.vault.set_withdraw_limits(&100, &50, &60);
    assert_eq!(t.vault.max_withdraw(&user), 50);
    assert_eq!(t.vault.max_redeem(&user), 50);
    t.vault.set_withdraw_limits_override(&true);
    assert_eq!(t.vault.max_withdraw(&user), 1_000);
    assert_eq!(t.vault.max_redeem(&user), 1_000);
    t.vault.set_withdraw_limits_override(&false);
    assert_eq!(t.vault.max_withdraw(&user), 50);
    assert_eq!(t.vault.max_redeem(&user), 50);
    assert_eq!(t.vault.get_period_withdrawn(), 0);
}
