//! The asks a terminal answers with the keyboard: a list of choices checked in place, and
//! a yes-or-no question.
//!
//! [`check`] and [`yes_or_no`] draw the ask, take keys, and return when the terminal answers
//! or leaves it, taking the ask off the screen so that what the caller prints next reads as
//! the answer. The terminal is in raw mode only while an ask is on screen: a guard puts the
//! mode and the cursor back whether the ask is answered, left, or unwound by a panic, and
//! `Ctrl-C` arrives as a key rather than a signal so that the guard runs before the process
//! ends.
//!
//! A marked [`Style`] hangs the ask on the rule of the steps (`◆` and `│`); a plain one
//! draws it as indented text.

use std::io::{self, IsTerminal, Read, Write};
use std::os::fd::{AsRawFd, RawFd};

use anyhow::{Context, Result};

use crate::style::Style;

/// One heading of the list, with the choices under it.
pub struct Group {
    pub title: String,
    pub choices: Vec<Choice>,
}

impl Group {
    fn has_a_check(&self) -> bool {
        self.choices.iter().any(|choice| choice.checked)
    }
}

/// What a group without a check says under its heading once `Enter` was pressed.
const CHECK_ONE: &str = "check at least one";

/// One choice of a [`Group`], and whether it is checked.
pub struct Choice {
    pub name: String,
    /// What the name stands for, shown beside it.
    pub label: String,
    pub checked: bool,
}

/// How the ask ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    /// The terminal confirmed the checks, which are in the groups.
    Confirmed,
    /// The terminal left the ask, and the checks say nothing.
    Left,
}

/// Whether a terminal is there to ask: the keys come from stdin and the ask is drawn on
/// stdout, so both are the terminal or there is no ask to make.
pub fn is_available() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

/// Asks a yes-or-no question on one line; only `y` or `yes` is yes.
pub fn confirm(question: &str) -> Result<bool> {
    let mut output = io::stdout();
    write!(output, "{question} [y/N] ")?;
    output.flush()?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("read the answer")?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Asks a yes-or-no question under a heading and the lines that explain it, answering
/// with whether it is yes. Yes is chosen first; `y` and `n` answer at once, `←` `→` move
/// between the two and `Enter` takes the one chosen. Leaving the ask is no.
pub fn yes_or_no(heading: &str, body: &[String], question: &str, style: &Style) -> Result<bool> {
    let mut input = io::stdin();
    let terminal = Terminal::raw(input.as_raw_fd())?;
    let mut carried = Vec::new();
    let mut yes = true;
    let mut drawn = 0;
    let answer = 'ask: loop {
        drawn = draw(&render_confirm(heading, body, question, yes, style), drawn)?;
        for key in read_keys(&mut input, &terminal, &mut carried)? {
            match key {
                Key::Yes => break 'ask true,
                Key::No | Key::Leave => break 'ask false,
                Key::Left | Key::Right | Key::Up | Key::Down => yes = !yes,
                Key::Confirm => break 'ask yes,
                Key::Toggle | Key::Other => {}
            }
        }
    };
    erase(drawn)?;
    drop(terminal);
    Ok(answer)
}

/// Asks which choices to check, writing the answer into `groups`. `Enter` confirms once
/// every group has a check; until then a group without one says so under its heading.
///
/// `title` says what is being chosen, and `hint` which keys do what.
pub fn check(groups: &mut [Group], title: &str, hint: &str, style: &Style) -> Result<Answer> {
    let total: usize = groups.iter().map(|group| group.choices.len()).sum();
    if total == 0 {
        return Ok(Answer::Confirmed);
    }
    let mut input = io::stdin();
    let terminal = Terminal::raw(input.as_raw_fd())?;
    let mut carried = Vec::new();
    let mut cursor = 0;
    let mut drawn = 0;
    let mut insisted = false;
    let answer = 'ask: loop {
        drawn = draw(&render(groups, title, hint, cursor, insisted, style), drawn)?;
        // A held key or a fast hand puts several presses in one read, so every key of the
        // read is taken before the list is drawn again.
        for key in read_keys(&mut input, &terminal, &mut carried)? {
            match key {
                Key::Up => cursor = (cursor + total - 1) % total,
                Key::Down => cursor = (cursor + 1) % total,
                Key::Toggle => {
                    let choice = at(groups, cursor);
                    choice.checked = !choice.checked;
                }
                Key::Confirm if groups.iter().all(Group::has_a_check) => {
                    break 'ask Answer::Confirmed;
                }
                Key::Confirm => insisted = true,
                Key::Leave => break 'ask Answer::Left,
                Key::Left | Key::Right | Key::Yes | Key::No | Key::Other => {}
            }
        }
    };
    erase(drawn)?;
    drop(terminal);
    Ok(answer)
}

/// The choice the cursor is on, counting through the groups in order.
fn at(groups: &mut [Group], cursor: usize) -> &mut Choice {
    groups
        .iter_mut()
        .flat_map(|group| group.choices.iter_mut())
        .nth(cursor)
        .expect("the cursor stays within the choices")
}

/// The lines of an ask: the heading, then the body hung on the rule. A marked style leads
/// with the rule that joins the ask to the step above it and ends the rule under it; a
/// plain one leads with a blank line, so that the ask stands apart from what was printed
/// before it and goes away with it.
fn framed(heading: &str, body: Vec<String>, style: &Style) -> Vec<String> {
    if style.is_marked() {
        let rule = style.dim(style.rule());
        let mut lines = vec![
            rule.clone(),
            format!("{} {heading}", style.accent(style.asking())),
        ];
        lines.extend(body.into_iter().map(|line| {
            if line.is_empty() {
                rule.clone()
            } else {
                format!("{rule}  {line}")
            }
        }));
        lines.push(style.dim(style.end()));
        lines
    } else {
        let mut lines = vec![String::new(), heading.to_owned()];
        lines.extend(body);
        lines
    }
}

/// The lines of the list as it stands, with the cursor on the choice it is on. Once
/// `insisted`, a group without a check says so under its heading.
fn render(
    groups: &[Group],
    title: &str,
    hint: &str,
    cursor: usize,
    insisted: bool,
    style: &Style,
) -> Vec<String> {
    let width = groups
        .iter()
        .flat_map(|group| group.choices.iter())
        .map(|choice| choice.name.len())
        .max()
        .unwrap_or_default();
    let marked = style.is_marked();
    let heading = if marked {
        format!("{}  {}", style.bold(title), style.dim(hint))
    } else {
        format!("{title}.  {hint}")
    };
    // A plain list keeps its marks in the first columns, where the rule would be.
    let indent = if marked { "" } else { "  " };
    let mut body = Vec::new();
    let mut index = 0;
    for group in groups {
        body.push(String::new());
        body.push(format!("{indent}{}", style.dim(&group.title)));
        if insisted && !group.has_a_check() {
            body.push(format!("{indent}{}", style.warn(CHECK_ONE)));
        }
        for choice in &group.choices {
            let pointer = if index == cursor {
                style.accent(style.cursor())
            } else {
                " ".to_owned()
            };
            let mark = if choice.checked {
                style.accent(style.checked())
            } else {
                style.dim(style.unchecked())
            };
            body.push(format!(
                "{pointer} {mark} {:<width$}   {}",
                choice.name,
                style.dim(&choice.label)
            ));
            index += 1;
        }
    }
    framed(&heading, body, style)
}

/// The lines of a yes-or-no question, with the answer chosen so far.
fn render_confirm(
    heading: &str,
    body: &[String],
    question: &str,
    yes: bool,
    style: &Style,
) -> Vec<String> {
    let choice = |chosen: bool, label: &str| {
        if chosen {
            format!("{} {label}", style.accent(style.checked()))
        } else {
            style.dim(&format!("{} {label}", style.unchecked()))
        }
    };
    let asked = format!("{question}  {}  {}", choice(yes, "Yes"), choice(!yes, "No"));
    let mut lines = Vec::new();
    if style.is_marked() {
        lines.push(String::new());
        lines.extend(body.iter().cloned());
        if !body.is_empty() {
            lines.push(String::new());
        }
        lines.push(asked);
    } else {
        lines.extend(body.iter().map(|line| format!("  {line}")));
        lines.push(String::new());
        lines.push(asked);
    }
    framed(heading, lines, style)
}

/// Writes the lines where the last draw of `drawn` rows put them, and answers with how
/// many rows are now on screen. A line wider than the terminal wraps onto more than one.
fn draw(lines: &[String], drawn: usize) -> Result<usize> {
    let columns = columns();
    let mut screen = String::new();
    if drawn > 0 {
        screen.push_str(&format!("\x1b[{drawn}A"));
    }
    screen.push_str("\r\x1b[0J");
    let mut rows = 0;
    for line in lines {
        screen.push_str(line);
        screen.push_str("\r\n");
        rows += rows_of(line, columns);
    }
    write(&screen)?;
    Ok(rows)
}

/// Takes the ask off the screen, so that what follows reads as the only answer.
fn erase(drawn: usize) -> Result<()> {
    if drawn == 0 {
        return Ok(());
    }
    write(&format!("\r\x1b[{drawn}A\x1b[0J"))
}

fn write(screen: &str) -> Result<()> {
    let mut output = io::stdout();
    output
        .write_all(screen.as_bytes())
        .context("draw the ask")?;
    output.flush().context("draw the ask")
}

/// How many columns the terminal on stdout has, when it says.
fn columns() -> Option<usize> {
    // SAFETY: winsize is plain integers, for which all zeros is a valid value.
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ writes one winsize into the pointer, which points at `size`.
    let read = unsafe { libc::ioctl(io::stdout().as_raw_fd(), libc::TIOCGWINSZ, &mut size) };
    (read == 0 && size.ws_col > 0).then_some(usize::from(size.ws_col))
}

/// How many rows a line takes on a terminal of `columns`.
fn rows_of(line: &str, columns: Option<usize>) -> usize {
    match columns {
        Some(columns) => width_of(line).div_ceil(columns).max(1),
        None => 1,
    }
}

/// How many columns a line takes: its escape sequences take none, and the wide characters
/// of East Asian scripts and emoji take two.
fn width_of(line: &str) -> usize {
    let mut width = 0;
    let mut chars = line.chars();
    while let Some(char) = chars.next() {
        if char == '\x1b' {
            // A CSI sequence runs to its final byte.
            if chars.next() == Some('[') {
                for byte in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&byte) {
                        break;
                    }
                }
            }
            continue;
        }
        width += match u32::from(char) {
            0x1100..=0x115f
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe30..=0xfe4f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1faff
            | 0x20000..=0x3fffd => 2,
            _ => 1,
        };
    }
    width
}

/// What one key press asks the list to do.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Up,
    Down,
    Left,
    Right,
    Toggle,
    Yes,
    No,
    Confirm,
    Leave,
    Other,
}

/// Reads the keys that have come, starting with the bytes `carried` over from the read
/// before. A read that fills the buffer part way through an escape sequence carries
/// that start over to the next read, where the rest of the sequence arrives.
fn read_keys(
    input: &mut impl Read,
    terminal: &impl Wait,
    carried: &mut Vec<u8>,
) -> Result<Vec<Key>> {
    let mut buffer = [0u8; 16];
    let mut read = carried.len();
    buffer[..read].copy_from_slice(carried);
    carried.clear();
    if read == 0 {
        read = input.read(&mut buffer).context("read a key")?;
        // A terminal that closed leaves the ask rather than reading forever.
        if read == 0 {
            return Ok(vec![Key::Leave]);
        }
    }
    // An escape sequence can arrive split across reads, over ssh among other places, and
    // its lone `Esc` would otherwise read as leaving the ask. A read that ends part way
    // through one waits a moment for the rest.
    if unfinished(&buffer[..read]) > 0 && read < buffer.len() {
        terminal.waits_for_the_rest()?;
        read += input.read(&mut buffer[read..]).context("read a key")?;
        terminal.waits_for_a_key()?;
    }
    if read == buffer.len() {
        let complete = read - unfinished(&buffer[..read]);
        carried.extend_from_slice(&buffer[complete..read]);
        read = complete;
    }
    Ok(keys_of(&buffer[..read]))
}

/// How many bytes at the end begin an escape sequence whose rest is still on its way.
fn unfinished(bytes: &[u8]) -> usize {
    match bytes {
        [.., 0x1b, b'[' | b'O'] => 2,
        [.., 0x1b] => 1,
        _ => 0,
    }
}

/// The keys one read spells, taken one after another.
fn keys_of(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let (key, read) = key_at(&bytes[at..]);
        keys.push(key);
        at += read;
    }
    keys
}

/// The key the bytes begin with, and how many of them it spells.
fn key_at(bytes: &[u8]) -> (Key, usize) {
    match bytes {
        // An arrow key, in both the normal and the application cursor mode a terminal
        // sends it in.
        [0x1b, b'[', b'A', ..] | [0x1b, b'O', b'A', ..] => (Key::Up, 3),
        [0x1b, b'[', b'B', ..] | [0x1b, b'O', b'B', ..] => (Key::Down, 3),
        [0x1b, b'[', b'C', ..] | [0x1b, b'O', b'C', ..] => (Key::Right, 3),
        [0x1b, b'[', b'D', ..] | [0x1b, b'O', b'D', ..] => (Key::Left, 3),
        [b'y' | b'Y', ..] => (Key::Yes, 1),
        [b'n' | b'N', ..] => (Key::No, 1),
        [b'k', ..] => (Key::Up, 1),
        [b'j', ..] => (Key::Down, 1),
        [b' ', ..] => (Key::Toggle, 1),
        [b'\r', ..] | [b'\n', ..] => (Key::Confirm, 1),
        // Ctrl-C, Ctrl-D, Esc on its own, and `q` all leave the ask.
        [0x03, ..] | [0x04, ..] | [b'q', ..] | [0x1b] => (Key::Leave, 1),
        // Another escape sequence runs to its final byte, so the key after it is read as
        // a key of its own.
        [0x1b, b'[' | b'O', rest @ ..] => (
            Key::Other,
            rest.iter()
                .position(|byte| (0x40..=0x7e).contains(byte))
                .map_or(bytes.len(), |at| at + 3),
        ),
        _ => (Key::Other, 1),
    }
}

/// The terminal mode the ask borrows, and the guard that gives it back.
struct Terminal {
    fd: RawFd,
    saved: libc::termios,
    raw: libc::termios,
}

/// How long a read waits for the keys.
trait Wait {
    /// Reads a key at a time, waiting for one.
    fn waits_for_a_key(&self) -> Result<()>;

    /// Reads whatever is already there, waiting a tenth of a second at most.
    fn waits_for_the_rest(&self) -> Result<()>;
}

impl Terminal {
    /// Takes the keys one at a time, without echo, and with `Ctrl-C` as a key.
    fn raw(fd: RawFd) -> Result<Self> {
        // SAFETY: termios is plain integers and arrays of them, for which all zeros is a
        // valid value.
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: tcgetattr writes one termios into the pointer, which points at `saved`.
        if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
            return Err(io::Error::last_os_error()).context("read the terminal mode");
        }
        let mut raw = saved;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        // SAFETY: tcsetattr only reads the termios that `raw` holds.
        if unsafe { libc::tcsetattr(fd, libc::TCSADRAIN, &raw) } != 0 {
            return Err(io::Error::last_os_error()).context("put the terminal in raw mode");
        }
        let terminal = Self { fd, saved, raw };
        write("\x1b[?25l")?;
        Ok(terminal)
    }

    fn timing(&self, min: u8, time: u8) -> Result<()> {
        let mut mode = self.raw;
        mode.c_cc[libc::VMIN] = min;
        mode.c_cc[libc::VTIME] = time;
        // SAFETY: tcsetattr only reads the termios that `mode` holds.
        if unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &mode) } != 0 {
            return Err(io::Error::last_os_error()).context("set how long the terminal waits");
        }
        Ok(())
    }
}

impl Wait for Terminal {
    fn waits_for_a_key(&self) -> Result<()> {
        self.timing(1, 0)
    }

    fn waits_for_the_rest(&self) -> Result<()> {
        self.timing(0, 1)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // SAFETY: tcsetattr only reads the termios that `self.saved` holds.
        unsafe { libc::tcsetattr(self.fd, libc::TCSADRAIN, &self.saved) };
        let mut output = io::stdout();
        let _ = output.write_all(b"\x1b[?25h");
        let _ = output.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups() -> Vec<Group> {
        vec![
            Group {
                title: "Runtime Adapter".to_owned(),
                choices: vec![
                    Choice {
                        name: "claude-code".to_owned(),
                        label: "Claude Code".to_owned(),
                        checked: true,
                    },
                    Choice {
                        name: "codex".to_owned(),
                        label: "Codex".to_owned(),
                        checked: false,
                    },
                ],
            },
            Group {
                title: "Execution Provider".to_owned(),
                choices: vec![Choice {
                    name: "tmux".to_owned(),
                    label: "tmux".to_owned(),
                    checked: true,
                }],
            },
        ]
    }

    /// A terminal with colors and a UTF-8 locale.
    fn marked() -> Style {
        Style::decide(
            |name| match name {
                "TERM" => Some("xterm-256color".to_owned()),
                "LANG" => Some("en_US.UTF-8".to_owned()),
                _ => None,
            },
            true,
            false,
        )
    }

    /// The lines as they read, without their colors.
    fn uncolored(lines: Vec<String>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| {
                let mut text = String::new();
                let mut chars = line.chars();
                while let Some(char) = chars.next() {
                    if char == '\x1b' {
                        chars.by_ref().find(|byte| *byte == 'm');
                    } else {
                        text.push(char);
                    }
                }
                text
            })
            .collect()
    }

    #[test]
    fn the_list_heads_each_group_and_marks_the_checks_and_the_cursor() {
        assert_eq!(
            render(&groups(), "Choose", "keys", 2, false, &Style::plain()),
            [
                "",
                "Choose.  keys",
                "",
                "  Runtime Adapter",
                "  [x] claude-code   Claude Code",
                "  [ ] codex         Codex",
                "",
                "  Execution Provider",
                "> [x] tmux          tmux",
            ]
        );
    }

    #[test]
    fn a_marked_list_hangs_on_the_rule_of_the_steps() {
        assert_eq!(
            uncolored(render(&groups(), "Choose", "keys", 0, false, &marked())),
            [
                "\u{2502}",
                "\u{25c6} Choose  keys",
                "\u{2502}",
                "\u{2502}  Runtime Adapter",
                "\u{2502}  \u{276f} \u{25c9} claude-code   Claude Code",
                "\u{2502}    \u{25cb} codex         Codex",
                "\u{2502}",
                "\u{2502}  Execution Provider",
                "\u{2502}    \u{25c9} tmux          tmux",
                "\u{2514}",
            ]
        );
    }

    /// A group left without a check says so under its heading once `Enter` was pressed,
    /// and only then.
    #[test]
    fn a_group_without_a_check_asks_for_one_once_enter_was_pressed() {
        let mut groups = groups();
        groups[1].choices[0].checked = false;
        assert!(groups[0].has_a_check());
        assert!(!groups[1].has_a_check());
        let plain = Style::plain();
        assert!(
            !render(&groups, "Choose", "keys", 0, false, &plain)
                .contains(&format!("  {CHECK_ONE}"))
        );
        assert_eq!(
            render(&groups, "Choose", "keys", 0, true, &plain)[7..],
            [
                "  Execution Provider".to_owned(),
                format!("  {CHECK_ONE}"),
                "  [ ] tmux          tmux".to_owned(),
            ]
        );
    }

    #[test]
    fn a_question_shows_its_lines_and_the_answer_chosen() {
        let body = ["key".to_owned(), "+ value".to_owned()];
        assert_eq!(
            uncolored(render_confirm(
                "Claude Code",
                &body,
                "Apply?",
                false,
                &marked()
            )),
            [
                "\u{2502}",
                "\u{25c6} Claude Code",
                "\u{2502}",
                "\u{2502}  key",
                "\u{2502}  + value",
                "\u{2502}",
                "\u{2502}  Apply?  \u{25cb} Yes  \u{25c9} No",
                "\u{2514}",
            ]
        );
        assert_eq!(
            render_confirm("Claude Code", &body, "Apply?", true, &Style::plain()),
            [
                "",
                "Claude Code",
                "  key",
                "  + value",
                "",
                "Apply?  [x] Yes  [ ] No",
            ]
        );
    }

    /// A line wider than the terminal takes as many rows as it wraps onto, and its colors
    /// take none.
    #[test]
    fn a_line_takes_the_rows_it_wraps_onto() {
        assert_eq!(rows_of("", Some(10)), 1);
        assert_eq!(rows_of("0123456789", Some(10)), 1);
        assert_eq!(rows_of("0123456789a", Some(10)), 2);
        assert_eq!(rows_of("\x1b[1;94m0123456789\x1b[0m", Some(10)), 1);
        assert_eq!(
            rows_of("\u{2502}  \u{3042}\u{3044}\u{3046}\u{3048}", Some(10)),
            2
        );
        assert_eq!(rows_of(&"x".repeat(100), None), 1);
    }

    #[test]
    fn the_keys_move_check_confirm_and_leave() {
        assert_eq!(keys_of(b"\x1b[A"), [Key::Up]);
        assert_eq!(keys_of(b"\x1b[B"), [Key::Down]);
        assert_eq!(keys_of(b"\x1bOA"), [Key::Up]);
        assert_eq!(keys_of(b"\x1bOB"), [Key::Down]);
        assert_eq!(keys_of(b"k"), [Key::Up]);
        assert_eq!(keys_of(b"j"), [Key::Down]);
        assert_eq!(keys_of(b" "), [Key::Toggle]);
        assert_eq!(keys_of(b"\r"), [Key::Confirm]);
        assert_eq!(keys_of(b"\n"), [Key::Confirm]);
        for leave in [&b"\x03"[..], b"\x04", b"\x1b", b"q"] {
            assert_eq!(keys_of(leave), [Key::Leave], "{leave:?}");
        }
        assert_eq!(keys_of(b"\x1b[C"), [Key::Right]);
        assert_eq!(keys_of(b"\x1b[D"), [Key::Left]);
        assert_eq!(keys_of(b"y"), [Key::Yes]);
        assert_eq!(keys_of(b"n"), [Key::No]);
        assert_eq!(keys_of(b"x"), [Key::Other]);
    }

    /// A held key or a fast hand puts several presses in one read, and every one of them
    /// counts.
    #[test]
    fn one_read_spells_every_key_it_holds() {
        assert_eq!(
            keys_of(b"\x1b[B\x1b[B \r"),
            [Key::Down, Key::Down, Key::Toggle, Key::Confirm]
        );
        // A sequence this list has no use for is passed over, and the key after it counts.
        assert_eq!(keys_of(b"\x1b[1;5D "), [Key::Other, Key::Toggle]);
        assert_eq!(keys_of(b"\x1b[<0;1;1M\r"), [Key::Other, Key::Confirm]);
    }

    /// An escape sequence split across reads is waited for rather than read as the `Esc`
    /// that leaves the ask.
    #[test]
    fn a_read_that_ends_part_way_through_a_sequence_waits_for_the_rest() {
        assert_eq!(unfinished(b"\x1b"), 1);
        assert_eq!(unfinished(b" \x1b"), 1);
        assert_eq!(unfinished(b"\x1b["), 2);
        assert_eq!(unfinished(b"\x1bO"), 2);
        assert_eq!(unfinished(b"\x1b[B"), 0);
        assert_eq!(unfinished(b" "), 0);
        assert_eq!(read(&[b"\x1b", b"[B"]), [vec![Key::Down]]);
    }

    /// A read that fills the buffer part way through a sequence carries its start over to
    /// the next read.
    #[test]
    fn a_sequence_cut_off_by_a_full_buffer_is_read_as_its_key() {
        let downs = b"\x1b[B".repeat(5);
        let cut = [&downs[..], b"\x1b"].concat();
        assert_eq!(read(&[&cut, b"[B"]), [vec![Key::Down; 5], vec![Key::Down]]);
        let cut = [&downs[..12], b"jj\x1b["].concat();
        assert_eq!(read(&[&cut, b"B"]), [vec![Key::Down; 6], vec![Key::Down]]);
        // An `Esc` that nothing follows still leaves the ask.
        let cut = [&downs[..15], b"\x1b"].concat();
        assert_eq!(read(&[&cut]), [vec![Key::Down; 5], vec![Key::Leave]]);
    }

    /// The keys each call of `read_keys` gives while the input brings `chunks`, one chunk
    /// a read, until the chunks and the bytes carried over run out.
    fn read(chunks: &[&[u8]]) -> Vec<Vec<Key>> {
        struct Chunks<'a>(std::slice::Iter<'a, &'a [u8]>);
        impl Read for Chunks<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let Some(chunk) = self.0.next() else {
                    return Ok(0);
                };
                buffer[..chunk.len()].copy_from_slice(chunk);
                Ok(chunk.len())
            }
        }
        struct Ready;
        impl Wait for Ready {
            fn waits_for_a_key(&self) -> Result<()> {
                Ok(())
            }
            fn waits_for_the_rest(&self) -> Result<()> {
                Ok(())
            }
        }
        let mut input = Chunks(chunks.iter());
        let mut carried = Vec::new();
        let mut reads = Vec::new();
        while input.0.len() > 0 || !carried.is_empty() {
            reads.push(read_keys(&mut input, &Ready, &mut carried).unwrap());
        }
        reads
    }

    #[test]
    fn a_toggle_writes_the_answer_into_the_group_the_cursor_is_in() {
        let mut groups = groups();
        let choice = at(&mut groups, 1);
        choice.checked = !choice.checked;
        assert!(groups[0].choices[1].checked);
    }
}
