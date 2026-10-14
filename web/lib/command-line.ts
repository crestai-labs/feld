// An argv as one line for a human to read, and the line read back as an argv. The
// quoting is a POSIX shell's, so a command the page shows can be copied into a terminal
// and reach the same argv; the CLI writes a command the same way.

// Characters every shell leaves alone.
const BARE = /^[A-Za-z0-9_@%+=:,./-]+$/;

// One shell word. A single quote inside the word closes, escapes, and reopens.
function shellQuote(word: unknown) {
  const text = String(word);
  return BARE.test(text) ? text : `'${text.replaceAll("'", "'\\''")}'`;
}

function commandLine(argv: readonly unknown[] | null | undefined) {
  return (argv || []).map(shellQuote).join(" ");
}

// The words of a line, with the quoting a shell removes removed: single quotes take
// their contents literally, double quotes keep the backslash escapes a shell reads, and
// a bare backslash escapes the next character. Adjacent parts make one word. `unclosed`
// is the quote left open at the end of the line, or null.
function readCommand(text: unknown) {
  const source = String(text);
  const argv: string[] = [];
  let unclosed: "'" | '"' | null = null;
  let word: string | null = null;
  let index = 0;
  while (index < source.length) {
    const char = source[index];
    if (/\s/.test(char)) {
      if (word !== null) argv.push(word);
      word = null;
      index += 1;
      continue;
    }
    if (word === null) word = "";
    if (char === "'") {
      const close = source.indexOf("'", index + 1);
      if (close === -1) unclosed = "'";
      const end = close === -1 ? source.length : close;
      word += source.slice(index + 1, end);
      index = end + 1;
      continue;
    }
    if (char === '"') {
      index += 1;
      while (index < source.length && source[index] !== '"') {
        if (source[index] === "\\" && /["\\$`]/.test(source[index + 1] || "")) index += 1;
        word += source[index];
        index += 1;
      }
      if (index >= source.length) unclosed = '"';
      index += 1;
      continue;
    }
    if (char === "\\" && index + 1 < source.length) {
      word += source[index + 1];
      index += 2;
      continue;
    }
    word += char;
    index += 1;
  }
  if (word !== null) argv.push(word);
  return { argv, unclosed };
}

function splitCommand(text: unknown) {
  return readCommand(text).argv;
}

// Why a Command field cannot be saved as an argv, or null.
function commandError(text: unknown) {
  const { argv, unclosed } = readCommand(text);
  if (unclosed) return `Command has an unclosed ${unclosed} quote.`;
  return argv.length ? null : "Command is required.";
}

export { shellQuote, commandLine, splitCommand, commandError };
