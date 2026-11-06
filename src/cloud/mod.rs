//! Sync between devices, over the network: an account (email and
//! password, see [`keys`]), a server that only ever holds ciphertext
//! ([`relay`]), and a round that sends this device's changes and folds in
//! the others' ([`run::sync_once`]). What changed and how changes merge is
//! [`crate::core::sync`].

pub mod cli;
pub mod client;
pub mod keys;
pub mod relay;
pub mod run;
