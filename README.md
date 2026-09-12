# notp

A native, GTK-based TOTP authenticator written in Rust. `notp` keeps your
two-factor authentication secrets in an encrypted vault on disk and generates
RFC 6238 time-based one-time passwords locally — no network, no cloud, no
third-party service.

## Features

- Encrypted local vault (AES-GCM 256, key derived with Argon2id).
- TOTP (RFC 6238) code generation with per-second countdown.
- Import accounts from QR codes:
  - decode a PNG / JPEG file containing a QR code,
  - scan a live QR code with a V4L2 camera,
  - Google Authenticator `otpauth-migration://` payloads (single entry and
    bulk, with a confirmation dialog for multi-entry imports).
- Bulk-import safety: a QR code that decodes to several entries must be
  confirmed before the entries are written to the vault.
- GTK 4 user interface with an open-or-create vault dialog, an account
  list with a detail / code panel, copy-to-clipboard and per-entry
  deletion, drag-and-drop reordering of entries (the order is persisted
  with the vault), and reliable error reporting.
- Auto-lock: the vault is locked automatically after 60 seconds without
  user input and can also be locked manually from the header bar.
- Clipboard copy via GTK's GDK clipboard with a transparent fallback to
  `wl-copy`, `xclip`, or `xsel` when the GDK helper is unavailable.
- Persistent settings (`~/.local/state/notp/settings.json`): the last
  opened vault path is remembered between launches.

## Requirements

- Rust **1.75** or newer.
- GTK **4.8** development libraries (`libgtk-4-dev`).
- A V4L2-compatible camera for the live scanner (optional).
- Read access to the video device for the user running `notp` (typically
  membership in the `video` group on Linux).

## Build

The project ships a `Makefile` that wraps `cargo`. The `camera` feature
is enabled by `make build`; it is **not** part of Cargo's default
features (the only default feature is `gtk`, required for the
graphical interface).

```sh
make build           # debug build (gtk + camera, via Makefile default)
make build-release   # release build
make clean           # cargo clean
```

Useful overrides:

```sh
make build FEATURES=""                              # only the gtk feature
make build FEATURES="gtk,camera"                    # explicit combination
make build CARGO=cargo-+1.80                        # use a specific cargo binary
cargo build                                         # cargo default: gtk only
cargo build --no-default-features                   # headless CLI helpers
cargo build --features camera                       # gtk + camera, plain cargo
```

The release binary is produced at `target/release/notp`.

## Install

`make install` builds the release artifact and installs the binary and
the `.desktop` entry:

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
`$(PREFIX)/share/icons/hicolor/scalable/apps/notp.svg` (or update the
`Icon=` key in `notp.desktop`).

## Usage

Launch `notp` from your application launcher or:

```sh
notp
```

On first run, pick a vault file path and choose a master passphrase. The
passphrase is used to derive the encryption key — it is never stored. On
subsequent runs, the last vault path is pre-selected; you only need to
enter the passphrase to unlock it.

If the chosen path does not point to an existing file, `notp` treats the
action as a vault creation: a confirmation field appears, the file is
created with a freshly generated salt and nonce, and the empty vault is
loaded.

To add an entry, click **Add entry** to fill in the issuer, account,
secret, digit count, hash algorithm, and period manually; or use the
import menu (**document-open-symbolic**) to:

- open an image file containing a QR code,
- start the live camera scanner (requires the `camera` feature).

If the QR code comes from a Google Authenticator migration payload,
`notp` decodes every entry it contains. For multi-entry payloads a
confirmation dialog lists the count before anything is written.

The desktop entry advertises the `otpauth` URI scheme
(`x-scheme-handler/otpauth`) so `notp` can be opened directly from
other applications.

### Camera selection

By default the scanner opens the first `/dev/video*` node that reports
`V4L2_CAP_VIDEO_CAPTURE`. Set the `NOTP_VIDEO_DEVICE` environment
variable to pick a specific device:

```sh
NOTP_VIDEO_DEVICE=/dev/video2 notp
```

The variable accepts any path that can be opened with `O_RDWR`.

## Storage

The vault is stored under the user data directory reported by the
`dirs` crate (XDG-style on Linux, e.g. `~/.local/share/notp/`). The file
holds only Argon2 parameters, an AES-GCM nonce, and the ciphertext —
never the plaintext secrets or the passphrase.

User settings (currently just the last opened vault path) live next to
the data tree under the state directory
(`$XDG_STATE_HOME/notp/settings.json`, falling back to
`$XDG_CONFIG_HOME`). Both the vault and the settings file are written
atomically through temporary siblings that are renamed into place, so a
crash mid-save cannot leave a partially written file.

## Security notes

- All cryptography uses vetted primitives: Argon2id for the KDF,
  AES-GCM-256 for authenticated encryption, HMAC-SHA1 / SHA-256 /
  SHA-512 for the TOTP step as specified in RFC 6238. Secrets are
  zeroized on drop where possible.
- The passphrase is the root of trust. There is no recovery mechanism —
  if you forget it, the vault cannot be decrypted.
- AAD binds the KDF parameters, the salt, and the nonce to the
  ciphertext, so modifying the file header will fail decryption rather
  than silently producing valid plaintext. The same logic catches any
  tampering of the ciphertext.
- The on-disk format is versioned (`NOTP1`, file version `1`); KDF
  parameters read from disk are clamped to a safe range before being
  used to avoid malicious values triggering a denial of service.
- The camera feature talks to `/dev/video*` directly via V4L2. It only
  runs on Linux and requires read access to the video device.

## Project layout

```
src/
  main.rs        # entry point, GTK feature gating
  ui.rs          # GTK 4 interface: vault dialog, account list, detail view
  storage.rs     # encrypted vault, accounts, atomic file writes
  crypto.rs      # Argon2id + AES-GCM seal / open helpers
  otp.rs         # TOTP code generation (RFC 6238) and Base32 helpers
  qr_import.rs   # otpauth URI and Google Authenticator migration parsing
  camera.rs      # optional V4L2 QR scanner (camera feature)
  settings.rs    # persistent application settings (last vault path)
```

## Tests

```sh
cargo test
```

Unit tests cover RFC 6238 test vectors, Base32 canonicalisation,
the protobuf migration decoder, the Argon2id + AES-GCM round trip, and
tamper detection on the vault format.

## License

Unless a `LICENSE` file is added, all rights are reserved by the project
author.
