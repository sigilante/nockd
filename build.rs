fn main() {
    // Expose the build's target triple so the CLI can record it in artifact identity.
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=NOCKD_DEFAULT_TARGET={target}");
}
