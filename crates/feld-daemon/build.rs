/// The Web UI in `web/dist` is embedded with rust-embed, whose file names change with each
/// Vite build, so a change there compiles this crate again.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../web/dist");
}
