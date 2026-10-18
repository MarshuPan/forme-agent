//! Configured-provider C24 evidence runner; never substitutes scripted models.
#![forbid(unsafe_code)]

// Keep the runner out of the library's shared export surface.
#[path = "../product_golden_runner.rs"]
mod product_golden_runner;

fn main() {
    std::process::exit(product_golden_runner::entry());
}
