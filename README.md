# notp

A native, GTK-based TOTP authenticator written in Rust. `notp` keeps your
two-factor authentication secrets in an encrypted vault on disk and generates
RFC 6238 time-based one-time passwords locally — no network, no cloud, no
third-party service.

## Features

- Encrypted local vault (AES-GCM 256, key derived with Argon2).
- TOTP (RFC 6238) code generation with countdown.
- Import accounts from `otpauth://` URIs:
  - paste a URI,
  - scan a QR code from a PNG / JPEG file,
  - scan a QR code with a V4L2 camera,
  - import a Google Authenticator `otpauth-migration` payload.
- Clipboard copy with automatic fallback when the clipboard helper is
  unavailable.
- GTK 4 user interface with create / unlock dialogs, an account list, and
  reliable error reporting.

## Requirements

- Rust **1.75** or newer.
- GTK **4.8** development libraries (`libgtk-4-dev`).
- A V4L2-compatible camera for the live scanner (optional).

## Build

The project ships a `Makefile` that wraps `cargo`. The `camera` feature is
enabled by default.

```sh
make build           # debug build
make build-release   # release build
make clean           # cargo clean
```

Useful overrides:

```sh
make build FEATURES=""              # build without the camera feature
make build FEATURES="camera,extra"  # build with several features
make build CARGO=cargo-+1.80        # use a specific cargo binary
```

The release binary is produced at `target/release/notp`.

## Install

`make install` builds the release artifact and installs the binary and the
`.desktop` entry:

```sh
sudo make install
```

Defaults:

| Item          | Path                                          |
| ------------- | --------------------------------------------- |
| `BINARY`      | `$(PREFIX)/bin/notp`                          |
| `.desktop`    | `$(PREFIX)/share/applications/notp.desktop`   |

With `PREFIX` defaulting to `/usr/local`. Override as needed:

```sh
sudo make install PREFIX=/usr
sudo make install DESTDIR=/tmp/staging PREFIX=/usr
sudo make uninstall
```

If you want a system icon, drop one in
`$(PREFIX)/share/icons/hicolor/scalable/apps/notp.svg` (or update the `Icon=`
key in `notp.desktop`).

## Usage

Launch `notp` from your application launcher or:

```sh
notp
```

On first run, create a vault by choosing a passphrase. The passphrase is
used to derive the encryption key — it is never stored. On subsequent runs
you unlock the vault with the same passphrase.

To add an account, use the import menu and either:

- paste an `otpauth://...` URI,
- select an image file containing a QR code,
- start the live camera scanner (requires the `camera` feature),
- paste a Google Authenticator migration URL.

The desktop entry advertises the `otpauth` URI scheme (`x-scheme-handler/otpauth`)
so `notp` can be opened directly from other applications.

## Storage

The vault is stored under the user data directory reported by the `dirs`
crate (XDG-style on Linux, e.g. `~/.local/share/notp/`). The file holds
only Argon2 parameters, an AES-GCM nonce, and the ciphertext — never the
plaintext secrets or the passphrase.

## Security notes

- All cryptography uses vetted primitives: Argon2 for KDF, AES-GCM-256 for
  authenticated encryption, HMAC-SHA1 for the TOTP step as specified in
  RFC 6238. Secrets are zeroized on drop where possible.
- The passphrase is the root of trust. There is no recovery mechanism — if
  you forget it, the vault cannot be decrypted.
- AAD binds the KDF parameters and nonce to the ciphertext, so modifying
  the header will fail decryption rather than silently producing valid
  plaintext.
- The camera feature talks to `/dev/video*` directly via V4L2. It only
  runs on Linux and requires read access to the video device.

## Project layout

```
src/
  main.rs        # entry point, GTK feature gating
  ui.rs          # GTK 4 interface: create / unlock dialogs, account list
  storage.rs     # encrypted vault, accounts, atomic file writes
  crypto.rs      # Argon2 + AES-GCM seal / open helpers
  otp.rs         # TOTP code generation (RFC 6238)
  qr_import.rs   # otpauth URI and Google Authenticator migration parsing
  camera.rs      # optional V4L2 QR scanner (camera feature)
```

## License

Unless a `LICENSE` file is added, all rights are reserved by the project
author.