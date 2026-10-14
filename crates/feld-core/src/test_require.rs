//! `FELD_TEST_REQUIRE`: the products whose tests fail, rather than step aside, on a
//! machine without them. CI names every product it installs, so a test that would pass
//! without running shows as a failure there. Without the variable, every test steps aside.

/// The variable, a comma-separated list of the names in [`PRODUCTS`].
pub const VARIABLE: &str = "FELD_TEST_REQUIRE";

/// The products a test can require.
pub const PRODUCTS: [&str; 4] = ["tmux", "herdr", "claude-code", "codex"];

/// Whether `FELD_TEST_REQUIRE` names `product`. A name outside [`PRODUCTS`] panics, so a
/// misspelt product fails rather than leaves its tests stepping aside.
pub fn required(product: &str) -> bool {
    names(&std::env::var(VARIABLE).unwrap_or_default(), product)
}

/// Whether the list `value` names `product`, panicking at a name outside [`PRODUCTS`].
fn names(value: &str, product: &str) -> bool {
    let mut named = false;
    for name in value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        assert!(
            PRODUCTS.contains(&name),
            "{VARIABLE} names {name}, which is none of {}",
            PRODUCTS.join(", ")
        );
        named |= name == product;
    }
    named
}

/// Steps a test aside for want of `product`, or fails it when `FELD_TEST_REQUIRE` names
/// the product. `missing` says what this machine lacks.
pub fn step_aside(product: &str, missing: &str) {
    assert!(
        !required(product),
        "{VARIABLE} names {product}, but this machine has {missing}"
    );
    eprintln!("skipping a {product} test: this machine has {missing}");
}

/// Whether this machine runs `tmux -V`, stepping the test aside when it does not.
pub fn tmux() -> bool {
    // Read the variable before anything else, so that a misspelt name fails here too.
    required("tmux");
    let present = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok_and(|output| output.status.success());
    if !present {
        step_aside("tmux", "no tmux on PATH");
    }
    present
}

/// The API key the tests of the real Claude Code run with, or nothing with the reason on
/// stderr. Those tests call the API, so they run only where `FELD_TEST_REQUIRE` names
/// `claude-code`, and there a machine without `claude` or the key fails them.
pub fn claude_code() -> Option<String> {
    real_runtime("claude-code", "claude", "ANTHROPIC_API_KEY")
}

/// The API key the tests of the real Codex run with, as [`claude_code`] answers for Claude
/// Code: only where `FELD_TEST_REQUIRE` names `codex`, failing without `codex` or
/// `OPENAI_API_KEY`.
pub fn codex() -> Option<String> {
    real_runtime("codex", "codex", "OPENAI_API_KEY")
}

/// The key in `variable` for the tests of the real Runtime `product`, whose executable is
/// `program`.
fn real_runtime(product: &str, program: &str, variable: &str) -> Option<String> {
    if !required(product) {
        eprintln!("skipping a {product} test: {VARIABLE} does not name {product}");
        return None;
    }
    let present = std::process::Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    assert!(
        present,
        "{VARIABLE} names {product}, but this machine has no {program} on PATH"
    );
    let key = std::env::var(variable).unwrap_or_default();
    assert!(
        !key.is_empty(),
        "{VARIABLE} names {product}, but this machine has no {variable}"
    );
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_names_each_product_it_holds() {
        assert!(names("tmux, herdr", "herdr"));
        assert!(names("tmux,", "tmux"));
        assert!(!names("tmux", "herdr"));
        assert!(!names("", "tmux"));
    }

    #[test]
    #[should_panic(
        expected = "FELD_TEST_REQUIRE names tmx, which is none of tmux, herdr, claude-code, codex"
    )]
    fn a_name_outside_the_products_fails() {
        names("tmux,tmx", "tmux");
    }
}
