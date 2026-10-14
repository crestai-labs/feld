use std::process::Command;

fn main() {
    let built = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let version = env!("CARGO_PKG_VERSION");
    let info = match git(&["symbolic-ref", "--short", "-q", "HEAD"]) {
        Some(branch) => format!("{version} (built {built}, branch {branch})"),
        None => format!("{version} (built {built})"),
    };
    println!("cargo:rustc-env=FELD_BUILD_VERSION={info}");

    // Paths are relative to this crate. `feld` is built again when the crates it embeds
    // change, so the built time follows them too.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../feld-daemon");
    println!("cargo:rerun-if-changed=../feld-core");
    println!("cargo:rerun-if-changed=../../web/dist");
    println!("cargo:rerun-if-changed=../../plugins");
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}
