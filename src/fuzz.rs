//! Deterministic fuzz / property coverage for conversion, BPS (fee/rate),
//! and overflow invariants (issue #74).
//!
//! Uses a fixed-seed LCG so every CI run replays the same inputs. On failure
//! the assertion messages include the seed and the minimized counterexample
//! fields so a regression fixture can be cut without re-searching.

#![cfg(test)]

extern crate std;

use crate::error::Error;
use crate::math::{
    convert_to_assets, convert_to_shares, mul_div, price_per_share, share_fraction_bps,
};
use crate::types::{BPS_DENOMINATOR, PRICE_SCALE};

/// Canonical seed for the #74 fuzz lane. Keep stable across CI runs.
pub const FUZZ_SEED: u64 = 0x0059_5646_3734_2026; // "YVF74" + year marker
const FUZZ_ITERS: usize = 2_048;

/// Numerical Recipes LCG — tiny, deterministic, no_std-friendly via `u64`.
struct SeedRng {
    state: u64,
    seed: u64,
}

impl SeedRng {
    fn new(seed: u64) -> Self {
        Self { state: seed, seed }
    }

    fn seed(&self) -> u64 {
        self.seed
    }

    fn next_u64(&mut self) -> u64 {
        // Knuth / Numerical Recipes multiplicative LCG.
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }

    fn next_u128(&mut self) -> u128 {
        let hi = self.next_u64() as u128;
        let lo = self.next_u64() as u128;
        (hi << 64) | lo
    }

    /// Mix of small, mid-range, and near-`u128::MAX` values so overflow and
    /// dust edges are hit without needing millions of iterations.
    fn gen_amount(&mut self) -> u128 {
        match self.next_u64() % 8 {
            0 => 0,
            1 => 1 + (self.next_u64() % 1_000) as u128,
            2 => 1 + (self.next_u64() % 1_000_000) as u128,
            3 => {
                let bits = 1 + (self.next_u64() % 64) as u32;
                self.next_u128() & ((1u128 << bits) - 1)
            }
            4 => u128::MAX - (self.next_u64() % 1_024) as u128,
            5 => (u64::MAX as u128) - (self.next_u64() % 1_024) as u128,
            6 => self.next_u128() >> (self.next_u64() % 64),
            _ => self.next_u128(),
        }
    }

    fn gen_positive(&mut self) -> u128 {
        let v = self.gen_amount();
        if v == 0 {
            1
        } else {
            v
        }
    }

    fn gen_bps(&mut self) -> u128 {
        match self.next_u64() % 5 {
            0 => 1,
            1 => BPS_DENOMINATOR,
            2 => 100,
            3 => 10_000_000,
            _ => 1 + (self.next_u64() % 100_000) as u128,
        }
    }
}

fn fail_ctx(seed: u64, label: &str, detail: &str) -> std::string::String {
    std::format!("fuzz seed=0x{seed:016X} [{label}]: {detail}")
}

/// mul_div either returns the floored product/divisor or a documented error.
#[test]
fn fuzz_mul_div_floor_or_safe_reject() {
    let mut rng = SeedRng::new(FUZZ_SEED);
    for i in 0..FUZZ_ITERS {
        let a = rng.gen_amount();
        let b = rng.gen_amount();
        let d = rng.gen_amount();
        match mul_div(a, b, d) {
            Err(Error::DivisionByZero) => {
                assert_eq!(
                    d,
                    0,
                    "{}",
                    fail_ctx(
                        rng.seed(),
                        "mul_div/div0",
                        &std::format!("i={i} a={a} b={b} d={d}")
                    )
                );
            }
            Err(Error::MathOverflow) => {
                assert!(
                    a.checked_mul(b).is_none(),
                    "{}",
                    fail_ctx(
                        rng.seed(),
                        "mul_div/overflow-false-positive",
                        &std::format!("i={i} a={a} b={b} d={d}")
                    )
                );
            }
            Ok(q) => {
                let product = a.checked_mul(b).expect(&fail_ctx(
                    rng.seed(),
                    "mul_div/ok-but-overflow",
                    &std::format!("i={i} a={a} b={b} d={d}"),
                ));
                assert_ne!(
                    d,
                    0,
                    "{}",
                    fail_ctx(
                        rng.seed(),
                        "mul_div/ok-with-zero-denom",
                        &std::format!("i={i}")
                    )
                );
                assert_eq!(
                    q,
                    product / d,
                    "{}",
                    fail_ctx(
                        rng.seed(),
                        "mul_div/floor",
                        &std::format!("i={i} a={a} b={b} d={d} q={q}")
                    )
                );
            }
            Err(other) => {
                panic!(
                    "{}",
                    fail_ctx(
                        rng.seed(),
                        "mul_div/unexpected-err",
                        &std::format!("i={i} err={other:?} a={a} b={b} d={d}")
                    )
                );
            }
        }
    }
}

/// Round-trip conservation: converting assets→shares→assets never mints value.
#[test]
fn fuzz_convert_round_trip_conserves_value() {
    let mut rng = SeedRng::new(FUZZ_SEED ^ 0xC011_C011);
    for i in 0..FUZZ_ITERS {
        let total_shares = rng.gen_positive();
        let total_assets = rng.gen_positive();
        let assets = rng.gen_amount();

        let shares = match convert_to_shares(assets, total_shares, total_assets) {
            Ok(s) => s,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!(
                "{}",
                fail_ctx(
                    rng.seed(),
                    "shares/unexpected",
                    &std::format!(
                        "i={i} err={e:?} assets={assets} ts={total_shares} ta={total_assets}"
                    )
                )
            ),
        };

        let back = match convert_to_assets(shares, total_shares, total_assets) {
            Ok(a) => a,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!(
                "{}",
                fail_ctx(
                    rng.seed(),
                    "assets/unexpected",
                    &std::format!(
                        "i={i} err={e:?} shares={shares} ts={total_shares} ta={total_assets}"
                    )
                )
            ),
        };

        assert!(
            back <= assets,
            "{}",
            fail_ctx(
                rng.seed(),
                "conservation/assets-roundtrip",
                &std::format!(
                    "i={i} assets={assets} shares={shares} back={back} ts={total_shares} ta={total_assets}"
                )
            )
        );
    }
}

/// Symmetric conservation: shares→assets→shares never inflates share count.
#[test]
fn fuzz_convert_shares_round_trip_conserves() {
    let mut rng = SeedRng::new(FUZZ_SEED ^ 0x5441_5245);
    for i in 0..FUZZ_ITERS {
        let total_shares = rng.gen_positive();
        let total_assets = rng.gen_positive();
        let shares = rng.gen_amount();

        let assets = match convert_to_assets(shares, total_shares, total_assets) {
            Ok(a) => a,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!(
                "{}",
                fail_ctx(
                    rng.seed(),
                    "assets/unexpected",
                    &std::format!("i={i} err={e:?}")
                )
            ),
        };

        let back = match convert_to_shares(assets, total_shares, total_assets) {
            Ok(s) => s,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!(
                "{}",
                fail_ctx(
                    rng.seed(),
                    "shares/unexpected",
                    &std::format!("i={i} err={e:?}")
                )
            ),
        };

        assert!(
            back <= shares,
            "{}",
            fail_ctx(
                rng.seed(),
                "conservation/shares-roundtrip",
                &std::format!(
                    "i={i} shares={shares} assets={assets} back={back} ts={total_shares} ta={total_assets}"
                )
            )
        );
    }
}

/// Monotonicity: more assets (same vault state) never mint fewer shares.
#[test]
fn fuzz_convert_to_shares_monotonic() {
    let mut rng = SeedRng::new(FUZZ_SEED ^ 0x4D4F_4E4F);
    for i in 0..FUZZ_ITERS {
        let total_shares = rng.gen_positive();
        let total_assets = rng.gen_positive();
        let a1 = rng.gen_amount();
        let a2 = rng.gen_amount();
        let (lo, hi) = if a1 <= a2 { (a1, a2) } else { (a2, a1) };

        let s_lo = match convert_to_shares(lo, total_shares, total_assets) {
            Ok(s) => s,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!(
                "{}",
                fail_ctx(rng.seed(), "mono/lo", &std::format!("{e:?}"))
            ),
        };
        let s_hi = match convert_to_shares(hi, total_shares, total_assets) {
            Ok(s) => s,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!(
                "{}",
                fail_ctx(rng.seed(), "mono/hi", &std::format!("{e:?}"))
            ),
        };

        assert!(
            s_lo <= s_hi,
            "{}",
            fail_ctx(
                rng.seed(),
                "monotonicity/shares",
                &std::format!(
                    "i={i} lo={lo} hi={hi} s_lo={s_lo} s_hi={s_hi} ts={total_shares} ta={total_assets}"
                )
            )
        );
    }
}

/// Monotonicity for redeem: more shares never redeem fewer assets.
#[test]
fn fuzz_convert_to_assets_monotonic() {
    let mut rng = SeedRng::new(FUZZ_SEED ^ 0xA55E_A55E);
    for i in 0..FUZZ_ITERS {
        let total_shares = rng.gen_positive();
        let total_assets = rng.gen_positive();
        let s1 = rng.gen_amount();
        let s2 = rng.gen_amount();
        let (lo, hi) = if s1 <= s2 { (s1, s2) } else { (s2, s1) };

        let a_lo = match convert_to_assets(lo, total_shares, total_assets) {
            Ok(a) => a,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!(
                "{}",
                fail_ctx(rng.seed(), "mono/lo", &std::format!("{e:?}"))
            ),
        };
        let a_hi = match convert_to_assets(hi, total_shares, total_assets) {
            Ok(a) => a,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!(
                "{}",
                fail_ctx(rng.seed(), "mono/hi", &std::format!("{e:?}"))
            ),
        };

        assert!(
            a_lo <= a_hi,
            "{}",
            fail_ctx(
                rng.seed(),
                "monotonicity/assets",
                &std::format!(
                    "i={i} lo={lo} hi={hi} a_lo={a_lo} a_hi={a_hi} ts={total_shares} ta={total_assets}"
                )
            )
        );
    }
}

/// BPS / "fee" path: a holder of ≤ total shares never reports > `bps` (100%).
#[test]
fn fuzz_share_fraction_bps_bounded() {
    let mut rng = SeedRng::new(FUZZ_SEED ^ 0xFEE5_FEE5);
    for i in 0..FUZZ_ITERS {
        let total_shares = rng.gen_positive();
        let shares = rng.gen_amount() % (total_shares.saturating_add(1)); // shares ∈ [0, total]
        let bps = rng.gen_bps();

        let frac = match share_fraction_bps(shares, total_shares, bps) {
            Ok(f) => f,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!(
                "{}",
                fail_ctx(
                    rng.seed(),
                    "bps/unexpected",
                    &std::format!("i={i} err={e:?} shares={shares} ts={total_shares} bps={bps}")
                )
            ),
        };

        assert!(
            frac <= bps,
            "{}",
            fail_ctx(
                rng.seed(),
                "bounded-fee/bps",
                &std::format!("i={i} frac={frac} bps={bps} shares={shares} total={total_shares}")
            )
        );
    }
}

/// Rate / price path: higher total_assets (fixed shares) never lowers pps.
#[test]
fn fuzz_price_per_share_monotonic_in_assets() {
    let mut rng = SeedRng::new(FUZZ_SEED ^ 0x5241_5445);
    for i in 0..FUZZ_ITERS {
        let total_shares = rng.gen_positive();
        let a1 = rng.gen_amount();
        let a2 = rng.gen_amount();
        let (lo, hi) = if a1 <= a2 { (a1, a2) } else { (a2, a1) };

        let p_lo = match price_per_share(total_shares, lo, PRICE_SCALE) {
            Ok(p) => p,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!("{}", fail_ctx(rng.seed(), "pps/lo", &std::format!("{e:?}"))),
        };
        let p_hi = match price_per_share(total_shares, hi, PRICE_SCALE) {
            Ok(p) => p,
            Err(Error::MathOverflow) => continue,
            Err(e) => panic!("{}", fail_ctx(rng.seed(), "pps/hi", &std::format!("{e:?}"))),
        };

        assert!(
            p_lo <= p_hi,
            "{}",
            fail_ctx(
                rng.seed(),
                "rate/pps-monotonic",
                &std::format!("i={i} lo={lo} hi={hi} p_lo={p_lo} p_hi={p_hi} ts={total_shares}")
            )
        );
    }
}

/// Empty-vault bootstrap: first depositor gets 1:1 shares; redeem of zero
/// total shares yields zero assets.
#[test]
fn fuzz_empty_vault_bootstrap_and_zero_totals() {
    let mut rng = SeedRng::new(FUZZ_SEED ^ 0xE7F7_E7F7);
    for i in 0..256 {
        let assets = rng.gen_amount();
        assert_eq!(
            convert_to_shares(assets, 0, 0),
            Ok(assets),
            "{}",
            fail_ctx(
                rng.seed(),
                "empty/shares",
                &std::format!("i={i} assets={assets}")
            )
        );
        assert_eq!(
            convert_to_shares(assets, 0, rng.gen_positive()),
            Ok(assets),
            "{}",
            fail_ctx(rng.seed(), "empty/shares-zero-ts", &std::format!("i={i}"))
        );
        let shares = rng.gen_amount();
        assert_eq!(
            convert_to_assets(shares, 0, rng.gen_amount()),
            Ok(0),
            "{}",
            fail_ctx(
                rng.seed(),
                "empty/assets",
                &std::format!("i={i} shares={shares}")
            )
        );
        assert_eq!(
            share_fraction_bps(shares, 0, BPS_DENOMINATOR),
            Ok(0),
            "{}",
            fail_ctx(rng.seed(), "empty/bps", &std::format!("i={i}"))
        );
        assert_eq!(
            price_per_share(0, rng.gen_amount(), PRICE_SCALE),
            Ok(PRICE_SCALE),
            "{}",
            fail_ctx(rng.seed(), "empty/pps", &std::format!("i={i}"))
        );
    }
}

/// Regression fixtures: known overflow / floor / bound edges pinned by seed.
#[test]
fn fuzz_regression_fixtures_overflow_and_bounds() {
    // Intermediate product overflows u128.
    assert_eq!(
        mul_div(u128::MAX, 2, 1),
        Err(Error::MathOverflow),
        "fixture: mul_div overflow"
    );
    assert_eq!(
        mul_div(1, 1, 0),
        Err(Error::DivisionByZero),
        "fixture: mul_div div0"
    );
    // Flooring: 1*1/2 == 0.
    assert_eq!(mul_div(1, 1, 2), Ok(0), "fixture: mul_div floor dust");
    // Full ownership reports exactly `bps`.
    assert_eq!(
        share_fraction_bps(1_000, 1_000, BPS_DENOMINATOR),
        Ok(BPS_DENOMINATOR),
        "fixture: full ownership == 100%"
    );
    // Partial ownership never exceeds bps.
    assert_eq!(
        share_fraction_bps(1, 3, BPS_DENOMINATOR),
        Ok(3333),
        "fixture: 1/3 floor bps"
    );
    // convert overflow path (assets * total_shares overflows).
    assert_eq!(
        convert_to_shares(u128::MAX, u128::MAX, 1),
        Err(Error::MathOverflow),
        "fixture: convert_to_shares overflow"
    );
    assert_eq!(
        convert_to_assets(u128::MAX, 1, u128::MAX),
        Err(Error::MathOverflow),
        "fixture: convert_to_assets overflow"
    );
}

/// End-to-end vault invariant under randomized deposit / yield / withdraw:
/// a full redeem never returns more assets than deposited + accrued yield
/// attributed to the position (vault keeps dust via floor rounding).
#[test]
fn fuzz_vault_deposit_withdraw_conserves_under_yield() {
    use crate::{YieldVault, YieldVaultClient};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::{StellarAssetClient, TokenClient};
    use soroban_sdk::{Address, Env};

    let mut rng = SeedRng::new(FUZZ_SEED ^ 0xAA11_7001);
    // Keep iteration count small: each pass spins up a Soroban Env and
    // writes a ledger snapshot. Eight seeded cases still hit deposit,
    // yield-funded withdraw, and dust edges without bloating CI artifacts.
    for i in 0..8 {
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

        let user = Address::generate(&env);
        // Bound amounts so token i128 and dust guards stay in range.
        let deposit: u128 = 1_000 + (rng.next_u64() % 5_000_000) as u128;
        let yield_amt: u128 = (rng.next_u64() % 2_000_000) as u128;
        token_admin.mint(&user, &(deposit as i128));

        let shares = vault.deposit(&user, &deposit);
        assert!(
            shares > 0,
            "{}",
            fail_ctx(
                rng.seed(),
                "vault/zero-shares",
                &std::format!("i={i} deposit={deposit}")
            )
        );

        if yield_amt > 0 {
            // Mock yield only bumps accounting; fund the vault token balance
            // so the eventual withdraw transfer can succeed (same pattern as
            // `test_deposit_yield_withdraw_round_trip`).
            token_admin.mint(&vault_address, &(yield_amt as i128));
            vault.accrue_yield(&yield_amt);
        }

        let before = token.balance(&user) as u128;
        let redeemed = vault.withdraw(&user, &shares);
        let after = token.balance(&user) as u128;

        assert_eq!(
            after,
            before + redeemed,
            "{}",
            fail_ctx(rng.seed(), "vault/token-delta", &std::format!("i={i}"))
        );
        // Solo depositor receives all yield; floor rounding still cannot
        // credit more than deposit + yield.
        assert!(
            redeemed <= deposit.saturating_add(yield_amt),
            "{}",
            fail_ctx(
                rng.seed(),
                "vault/conservation",
                &std::format!(
                    "i={i} deposit={deposit} yield={yield_amt} shares={shares} redeemed={redeemed}"
                )
            )
        );
        assert_eq!(
            vault.balance_of(&user),
            0,
            "{}",
            fail_ctx(rng.seed(), "vault/residual-shares", &std::format!("i={i}"))
        );
    }
}
