//! Library crate for `notp`.
//!
//! Exposes the storage, crypto, OTP, QR import, settings, camera and UI modules
//! so they can be shared between the GUI binary (`src/bin/gui.rs`) and the CLI
//! binary (`src/bin/cli.rs`).

pub mod camera;
pub mod crypto;
pub mod otp;
pub mod qr_import;
pub mod settings;
pub mod storage;

#[cfg(feature = "gtk")]
pub mod ui;
