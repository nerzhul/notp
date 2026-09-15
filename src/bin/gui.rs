//! GUI entry point for `notp`.
//!
//! This binary simply forwards into the GTK UI module exposed by the library.

fn main() {
    notp::ui::run();
}
