//! Error definitions for the YieldVault contract.
//!
//! Each variant maps to a stable `u32` code so that callers and off-chain
//! tooling can rely on the numbering across releases. Append new variants with
//! the next free number rather than renumbering existing ones.

use soroban_sdk::contracterror;

/// Errors that can be returned by the YieldVault contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The contract has already been initialized.
    AlreadyInitialized = 1,
    /// The contract has not been initialized yet.
    NotInitialized = 2,
    /// An arithmetic operation overflowed the supported integer range.
    MathOverflow = 3,
    /// A division by zero was attempted.
    DivisionByZero = 4,
    /// A zero amount was supplied where a positive amount is required.
    ZeroAmount = 5,
    /// The operation would mint or burn zero shares.
    ZeroShares = 6,
    /// The caller does not hold enough shares for the requested operation.
    InsufficientShares = 7,
    /// The deposit amount is below the vault's configured minimum.
    BelowMinimumDeposit = 8,
    /// The vault is paused and is not accepting new deposits.
    Paused = 9,
    /// The provided Wasm hash does not match the admin-approved expected hash.
    WasmHashMismatch = 10,
}
