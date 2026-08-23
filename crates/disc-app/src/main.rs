//! Placeholder so the workspace builds. Contents are owned by bead `discr-bih`.
//!
//! This crate is intentionally NOT in the workspace `default-members`: it will
//! pull macroquad, whose system dependencies may be missing on a build box, and
//! that must never be able to break `make core-check`.

fn main() {
    println!("disc-app: not implemented yet (discr-bih)");
}
