mod camera;
mod crypto;
mod otp;
mod qr_import;
mod settings;
mod storage;
#[cfg(feature = "gtk")]
mod ui;

#[cfg(feature = "gtk")]
fn main() {
    ui::run();
}

#[cfg(not(feature = "gtk"))]
fn main() {}
