//! CLI entry point for `notp`.
//!
//! Phase 1.1 only ships the scaffolding (clap argument parsing and a stub
//! dispatch table). The actual subcommands (`add`, `list`, `show`, ...) are
//! implemented in phase 1.2 of `.kilo/PLAN.md`.

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "notp",
    version,
    about = "Native TOTP authenticator with an optional GTK interface",
    long_about = None
)]
struct Cli {
    /// Path to the encrypted vault file. Defaults to the standard location.
    #[arg(long, global = true)]
    vault: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Add a new OTP entry (interactive in 1.2).
    Add {
        #[arg(long)]
        issuer: Option<String>,
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        secret: Option<String>,
    },
    /// List all entries stored in the vault.
    List,
    /// Show the current code for a single entry.
    Show {
        /// Issuer, account name or UUID of the entry.
        target: String,
    },
    /// Remove an entry from the vault.
    Remove {
        /// Issuer, account name or UUID of the entry.
        target: String,
    },
    /// Edit an existing entry.
    Edit {
        /// Issuer, account name or UUID of the entry.
        target: String,
    },
    /// Import an entry from a QR code image (file path).
    Import {
        #[arg(long = "qr")]
        qr: std::path::PathBuf,
    },
    /// Change the master password of an existing vault.
    ChangePassword,
    /// Generate a QR code (PNG) from an existing entry.
    GenQr {
        /// UUID of the entry.
        uuid: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            eprintln!(
                "notp-cli: no subcommand provided. Phase 1.2 will implement the \
                 dispatch. Run with --help to list the available commands."
            );
            std::process::exit(2);
        }
        Some(Command::Add { .. })
        | Some(Command::List)
        | Some(Command::Show { .. })
        | Some(Command::Remove { .. })
        | Some(Command::Edit { .. })
        | Some(Command::Import { .. })
        | Some(Command::ChangePassword)
        | Some(Command::GenQr { .. }) => {
            eprintln!("notp-cli: subcommand recognised but not yet implemented (phase 1.2).");
            std::process::exit(1);
        }
    }
}
