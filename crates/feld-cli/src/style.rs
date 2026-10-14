//! How the CLI marks what it shows a person: a color for each role, and a few symbols.
//!
//! The colors are the terminal's own 16, fixed by role, so that the theme decides the hue;
//! bright blue is Feld's. Output that is not a terminal, `NO_COLOR`, and `--json` carry
//! neither colors nor symbols, and `CLICOLOR_FORCE` asks for them anyway. A locale that is
//! not UTF-8 takes the ASCII stand-ins of the symbols.

use std::io::{self, IsTerminal};

/// The marks one output carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    color: bool,
    dim_256: bool,
    unicode: bool,
}

impl Style {
    /// The marks of what this process prints on stdout.
    pub fn stdout(json: bool) -> Self {
        Self::decide(
            |name| std::env::var(name).ok(),
            io::stdout().is_terminal(),
            json,
        )
    }

    /// Neither colors nor symbols.
    pub fn plain() -> Self {
        Self {
            color: false,
            dim_256: false,
            unicode: false,
        }
    }

    /// The Style the environment `variable` reads and the kind of output decide.
    pub fn decide(variable: impl Fn(&str) -> Option<String>, terminal: bool, json: bool) -> Self {
        let set = |name: &str| variable(name).is_some_and(|value| !value.is_empty());
        let term = variable("TERM").unwrap_or_default();
        let forced =
            variable("CLICOLOR_FORCE").is_some_and(|value| !value.is_empty() && value != "0");
        let color = !json
            && !set("NO_COLOR")
            && (forced || (terminal && !term.is_empty() && term != "dumb"));
        if !color {
            return Self::plain();
        }
        let colorterm = variable("COLORTERM").unwrap_or_default();
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
            .into_iter()
            .find_map(|name| variable(name).filter(|value| !value.is_empty()))
            .unwrap_or_default()
            .to_ascii_lowercase();
        Self {
            color,
            dim_256: term.contains("256color")
                || matches!(colorterm.as_str(), "truecolor" | "24bit"),
            unicode: locale.contains("utf-8") || locale.contains("utf8"),
        }
    }

    /// Whether the output is marked at all; a plain one is read as `key: value` lines.
    pub fn is_marked(&self) -> bool {
        self.color
    }

    fn paint(&self, sgr: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{sgr}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    /// Feld's blue: the wordmark, the step being answered, the cursor, and a check.
    pub fn accent(&self, text: &str) -> String {
        self.paint("1;94", text)
    }

    /// Success, and a value being written.
    pub fn ok(&self, text: &str) -> String {
        self.paint("32", text)
    }

    /// Something to notice.
    pub fn warn(&self, text: &str) -> String {
        self.paint("33", text)
    }

    /// A refusal, a failure, and a value being replaced.
    pub fn error(&self, text: &str) -> String {
        self.paint("31", text)
    }

    /// Rules, headings, keys, and hints, which the values stand out against.
    pub fn dim(&self, text: &str) -> String {
        // Faint (SGR 2) is ignored by some terminals, so dim is a gray.
        self.paint(if self.dim_256 { "38;5;242" } else { "90" }, text)
    }

    pub fn bold(&self, text: &str) -> String {
        self.paint("1", text)
    }

    fn symbol(&self, unicode: &'static str, ascii: &'static str) -> &'static str {
        if self.unicode { unicode } else { ascii }
    }

    /// The wave of the wordmark.
    pub fn wave(&self) -> &'static str {
        self.symbol("\u{223f}", "~")
    }

    /// A step already answered.
    pub fn answered(&self) -> &'static str {
        self.symbol("\u{25c7}", "-")
    }

    /// The step being answered.
    pub fn asking(&self) -> &'static str {
        self.symbol("\u{25c6}", "*")
    }

    /// The rule the steps hang on.
    pub fn rule(&self) -> &'static str {
        self.symbol("\u{2502}", "|")
    }

    /// Where the rule ends.
    pub fn end(&self) -> &'static str {
        self.symbol("\u{2514}", "-")
    }

    pub fn cursor(&self) -> &'static str {
        self.symbol("\u{276f}", ">")
    }

    pub fn checked(&self) -> &'static str {
        self.symbol("\u{25c9}", "[x]")
    }

    pub fn unchecked(&self) -> &'static str {
        self.symbol("\u{25cb}", "[ ]")
    }

    pub fn success(&self) -> &'static str {
        self.symbol("\u{2713}", "+")
    }

    pub fn failure(&self) -> &'static str {
        self.symbol("\u{2717}", "x")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(terminal: bool, json: bool, variables: &[(&str, &str)]) -> Style {
        Style::decide(
            |name| {
                variables
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            },
            terminal,
            json,
        )
    }

    const TERMINAL: [(&str, &str); 2] = [("TERM", "xterm-256color"), ("LANG", "en_US.UTF-8")];

    #[test]
    fn a_terminal_is_marked_and_anything_else_is_plain() {
        let marked = style(true, false, &TERMINAL);
        assert!(marked.is_marked());
        assert_eq!(marked.accent("Feld"), "\x1b[1;94mFeld\x1b[0m");
        assert_eq!(marked.dim("key"), "\x1b[38;5;242mkey\x1b[0m");
        assert_eq!(marked.checked(), "\u{25c9}");

        for plain in [
            style(false, false, &TERMINAL),
            style(true, true, &TERMINAL),
            style(true, false, &[TERMINAL[0], TERMINAL[1], ("NO_COLOR", "1")]),
            style(true, false, &[("TERM", "dumb")]),
        ] {
            assert_eq!(plain, Style::plain());
            assert_eq!(plain.ok("done"), "done");
        }
    }

    #[test]
    fn clicolor_force_marks_what_is_not_a_terminal_but_not_json() {
        let forced = [TERMINAL[0], TERMINAL[1], ("CLICOLOR_FORCE", "1")];
        assert!(style(false, false, &forced).is_marked());
        assert!(!style(false, true, &forced).is_marked());
        assert!(!style(false, false, &[TERMINAL[0], ("CLICOLOR_FORCE", "0")]).is_marked());
    }

    #[test]
    fn sixteen_colors_and_a_locale_that_is_not_utf8_take_the_plainer_marks() {
        let sixteen = style(true, false, &[("TERM", "xterm"), ("LC_ALL", "C")]);
        assert!(sixteen.is_marked());
        assert_eq!(sixteen.dim("key"), "\x1b[90mkey\x1b[0m");
        assert_eq!(sixteen.checked(), "[x]");
        assert_eq!(sixteen.rule(), "|");
        // COLORTERM speaks for a terminal whose TERM does not say 256 colors.
        let truecolor = style(
            true,
            false,
            &[("TERM", "xterm"), ("COLORTERM", "truecolor")],
        );
        assert_eq!(truecolor.dim("key"), "\x1b[38;5;242mkey\x1b[0m");
    }
}
