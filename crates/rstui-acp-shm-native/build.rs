//! napi-rs link setup: lets the `cdylib` leave `napi_*` symbols
//! undefined (resolved by the Node/Bun host at `dlopen`), incl. macOS
//! `-undefined dynamic_lookup`. (Inner doc so the crate-wide
//! `missing_docs` lint is satisfied for this build-script target too.)
fn main() {
    napi_build::setup();
}
