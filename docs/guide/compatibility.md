# Compatibility

During 0.x, Feld protects the user's data, and changes the shape of operations (CLI and API) with notice.

## Guarantees

| Target | Guarantee |
|---|---|
| store (data such as history, Actors, and Messages) | A new version automatically migrates a store created by at least the two previous minor versions. With an older store, it stops without breaking data and explains how to back it up |
| `FELD_HOME` layout (Plugin location, daemon settings, per-Session files, store) | When it changes, the new daemon migrates it on startup. The user only needs to run `feld init` |
| launchSpec (the recipe for starting an Actor; stored in the store) | Same as above |
| What Feld writes to the Runtime config root (Claude Code and Codex skills and hooks) | Rerunning `feld init` replaces them with the new version's |
| CLI commands and options, Control API, Runtime API | May change in a minor version. Changes come with migration steps under `### Breaking` in the [Release notes](https://github.com/crestai-labs/feld/releases) |
| Plugin protocol | Same as above. Bundled Plugins are aligned with the core by `feld init` |

Changes to the `FELD_HOME` layout, launchSpec, or what is written to the Runtime config root are also recorded under `### Breaking` in the Release notes.

Patch versions (such as 0.1.0 → 0.1.1) contain only compatible changes.

## Upgrading

1. If the [Release notes](https://github.com/crestai-labs/feld/releases) of the target version have a `### Breaking` section, follow the migration steps there
2. Run `brew upgrade feld` for a Homebrew install, or the install script again, to replace `feld` and the bundled Plugin executables with the new version. The script takes the version from `FELD_VERSION` (e.g. `FELD_VERSION=v0.2.0`); if you installed to a different location with `FELD_INSTALL_DIR`, pass the same value

   ```sh
   curl -fsSL https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh | sh
   ```

3. Run `feld init` again and choose the same Plugins as before. `feld init` places the chosen Plugins in `$FELD_HOME/plugins/` again, replaces Feld's skills and hooks in the Runtime config root with the new version's, and restarts the daemon. The new daemon migrates the store on startup

   ```sh
   feld init
   ```

Sessions keep running on the Provider while the daemon is stopped, and the new daemon takes them over as they are, so you can upgrade without stopping Agents. Claude Code instances that are already open use the new skills and hooks after `/reload-plugins`. Check the upgraded version with `version` in `feld daemon status`, and how well the environment is aligned with `feld doctor`. After step 2 and until you run `feld init`, the running daemon and the installed Plugins remain at the old version, commands that talk to the daemon print a warning to standard error, and `daemon` and `home` in `feld doctor` show `fail`.

When building from source, run `feld init` after rebuilding ([CONTRIBUTING.md](../../CONTRIBUTING.md#development-setup)).

## version

Versions follow [Semantic Versioning](https://semver.org/). During 0.x, the minor version goes up for compatibility-breaking changes and major feature additions, and the patch version goes up for fixes only.

1.0 fixes the shape of operations (CLI and API). 1.0 ships after multi-Node support and the shape of external Plugins are settled.
