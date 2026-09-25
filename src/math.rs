//! Pure share/asset conversion math for the YieldVault.
//!
//! All functions round down toward zero, which keeps rounding error in the
//! vault's favor and prevents depositors from extracting more value than they
//! contributed. Multiplication is checked to avoid silent overflow in
//! intermediate products.
//!
//! **Note on aggregates:** Aggregate totals (`total_shares`, `total_assets`,
//! user balances) in `lib.rs` use saturating arithmetic (per
//! [ADR 0026]), capping at `u128::MAX` / flooring at `0` rather than
//! returning [`Error::MathOverflow`]. This module's `mul_div` helper continues
//! to use checked multiplication for its intermediate product, where overflow
//! represents a genuine arithmetic error.
//!
//! [ADR 0026]: ../../docs/adr/0026-prefer-saturating-math-for-aggregates.md

use crate::error::Error;

/// Computes `a * b / denominator`, rounding the result down toward zero.
///
/// Multiplication is checked so that an intermediate product exceeding the
/// `u128` range returns [`Error::MathOverflow`] rather than wrapping. A zero
/// `denominator` returns [`Error::DivisionByZero`].
pub fn mul_div(a: u128, b: u128, denominator: u128) -> Result<u128, Error> {
    if denominator == 0 {
        return Err(Error::DivisionByZero);
    }
    let product = a.checked_mul(b).ok_or(Error::MathOverflow)?;
    Ok(product / denominator)
}

/// Converts an amount of underlying `assets` into vault shares.
///
/// When the vault is empty (`total_shares == 0`), the first depositor receives
/// shares one-to-one with the assets supplied, bootstrapping the exchange rate.
/// Otherwise shares are minted proportionally: `assets * total_shares /
/// total_assets`, rounding down so the vault never mints more value than it
/// receives.
pub fn convert_to_shares(
    assets: u128,
    total_shares: u128,
    total_assets: u128,
) -> Result<u128, Error> {
    if total_shares == 0 || total_assets == 0 {
        return Ok(assets);
    }
    mul_div(assets, total_shares, total_assets)
}

/// Computes the value of a single share in underlying assets, scaled by
/// `scale` to preserve precision (since integer division would otherwise
/// truncate fractional share prices).
///
/// Returns `scale` when the vault is empty, reflecting the one-to-one
/// bootstrap exchange rate, and rounds down otherwise.
pub fn price_per_share(total_shares: u128, total_assets: u128, scale: u128) -> Result<u128, Error> {
    if total_shares == 0 {
        return Ok(scale);
    }
    mul_div(total_assets, scale, total_shares)
}

/// Computes what fraction of the vault `shares` represents, expressed in
/// basis points (`bps`, where `10_000` bps == 100%).
///
/// Returns zero when no shares exist, and rounds down otherwise so the figure
/// never overstates an account's claim on the vault.
pub fn share_fraction_bps(shares: u128, total_shares: u128, bps: u128) -> Result<u128, Error> {
    if total_shares == 0 {
        return Ok(0);
    }
    mul_div(shares, bps, total_shares)
}

/// Converts an amount of vault `shares` into the underlying assets they are
/// redeemable for: `shares * total_assets / total_shares`, rounding down.
///
/// When no shares exist the result is zero, since there is no claim on the
/// vault's assets.
pub fn convert_to_assets(
    shares: u128,
    total_shares: u128,
    total_assets: u128,
) -> Result<u128, Error> {
    if total_shares == 0 {
        return Ok(0);
    }
    mul_div(shares, total_assets, total_shares)
}

/// Computes simple (non-compounding) yield for `assets` over `elapsed_secs` at
/// an annual rate of `rate_bps` basis points.
///
/// Formula: `assets * rate_bps * elapsed_secs / (BPS_DENOMINATOR * SECONDS_PER_YEAR)`,
/// rounding down. Returns zero when any input factor is zero. Intermediate
/// products use checked multiplication so overflow returns
/// [`Error::MathOverflow`] rather than wrapping.
pub fn simple_yield(assets: u128, rate_bps: u32, elapsed_secs: u64) -> Result<u128, Error> {
    if assets == 0 || rate_bps == 0 || elapsed_secs == 0 {
        return Ok(0);
    }
    let rate = u128::from(rate_bps);
    let elapsed = u128::from(elapsed_secs);
    let numerator = assets
        .checked_mul(rate)
        .ok_or(Error::MathOverflow)?
        .checked_mul(elapsed)
        .ok_or(Error::MathOverflow)?;
    let denominator = crate::types::BPS_DENOMINATOR
        .checked_mul(u128::from(crate::types::SECONDS_PER_YEAR))
        .ok_or(Error::MathOverflow)?;
    Ok(numerator / denominator)
}
