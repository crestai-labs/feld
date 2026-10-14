---
name: feld
description: Attach the current Claude Code runtime to Feld.
argument-hint: attach [ACTOR]
---

# Feld

When you opened this skill for a Message from Feld, go on with what the Message asks, such as its `feld` command, without attaching.

Otherwise, including `/feld attach`, attach this Claude Code runtime now with the Bash tool, then tell the person what it printed. The arguments are `$ARGUMENTS`. When they name an Actor after `attach`, as in `/feld attach leader`, run `feld actor attach <ACTOR> --self --runtime claude-code` with that name as `<ACTOR>`. Without a name, run `feld actor attach --self --runtime claude-code`.
