import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { test } from "vitest";
import { shellQuote, commandLine, splitCommand, commandError } from "./command-line.ts";

test("command-line", () => {
  // Words made only of characters every shell leaves alone stay bare.
  assert.equal(shellQuote("echo"), "echo");
  assert.equal(shellQuote("--project=10"), "--project=10");
  assert.equal(shellQuote("/tmp/triage.py"), "/tmp/triage.py");

  // Everything else is quoted, and a single quote closes, escapes, and reopens.
  assert.equal(shellQuote(""), "''");
  assert.equal(shellQuote("two  spaces"), "'two  spaces'");
  assert.equal(shellQuote("it's"), "'it'\\''s'");
  assert.equal(shellQuote('printf "Hello from Feld\\n"'), `'printf "Hello from Feld\\n"'`);

  assert.equal(commandLine(["echo", "hello"]), "echo hello");
  assert.equal(commandLine(["sh", "-c", 'printf "Hello from Feld\\n"']), `sh -c 'printf "Hello from Feld\\n"'`);
  assert.equal(commandLine([]), "");

  // The words a reader writes, with the quoting a shell removes removed. Adjacent parts
  // make one word.
  assert.deepEqual(splitCommand("echo hello"), ["echo", "hello"]);
  assert.deepEqual(splitCommand("  echo   hello  "), ["echo", "hello"]);
  assert.deepEqual(splitCommand(`sh -c 'cd /tmp && ls'`), ["sh", "-c", "cd /tmp && ls"]);
  assert.deepEqual(splitCommand(`echo "two  spaces"`), ["echo", "two  spaces"]);
  assert.deepEqual(splitCommand(`echo a"b"c`), ["echo", "abc"]);
  assert.deepEqual(splitCommand(`echo \\ x`), ["echo", " x"]);
  assert.deepEqual(splitCommand(""), []);

  // A Command field is saved only when it reads as a non-empty argv with every quote closed.
  assert.equal(commandError(`sh -c 'echo hi'`), null);
  assert.equal(commandError(`sh -c 'echo hi`), "Command has an unclosed ' quote.");
  assert.equal(commandError(`echo "hi`), 'Command has an unclosed " quote.');
  assert.equal(commandError(`echo "it's"`), null);
  assert.equal(commandError("  "), "Command is required.");

  // The acceptance the display owes a reader: what the page shows is what a shell splits
  // back into the argv it was written from, and what the page reads back is that same argv.
  const ARGVS = [
    ["echo", "hello"],
    ["sh", "-c", 'printf "Hello from Feld\\n"'],
    ["sh", "-c", "cd /tmp && ls; echo done"],
    ["say", "it's a $HOME `date`"],
    ["echo", "", "two  spaces", "tab\there"],
    ["echo", "back\\slash", "*", "~", "#comment"],
  ];

  for (const argv of ARGVS) {
    const line = commandLine(argv);
    const printed = execFileSync("/bin/sh", ["-c", `printf '%s\\0' ${line}`], { encoding: "utf8" });
    assert.deepEqual(printed.split("\0").slice(0, -1), argv, line);
    assert.deepEqual(splitCommand(line), argv, line);
  }
});
