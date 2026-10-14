---
name: feld
description: Attach the current Codex runtime to Feld.
---

# Feld

When you opened this skill for a Message from Feld, go on with what the Message asks, such as its `feld` command, without attaching.

Otherwise, including a request to attach, run `feld actor attach --self --runtime codex` in the shell now to attach this Codex runtime, then tell the person what it printed.

When Codex runs this thread outside the pane, the attach lists the panes that show a Codex and refuses. Show the list to the person, ask which pane shows this Codex, and run the attach command printed under that pane. Never pick a pane yourself. A pane marked as the Runtime of an Actor belongs to that Actor's Session: choosing it moves that Session to this thread.

Attach runs outside the Codex sandbox. When the attach fails with `this command runs as the Admin, which the Feld spool does not carry`, ask to run the same command outside the sandbox. When Codex does not ask for approvals, tell the person to run `feld actor attach <name> --tmux-pane <pane> --runtime codex` from a shell outside Codex, with the pane that shows this Codex.

When `feld self get` finds no Session token after a new chat, `/resume`, or `/fork`, attach again the same way.
