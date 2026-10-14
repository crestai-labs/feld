---
name: release
description: Steps to ship a new version of Feld (feld). Covers, in one flow, choosing the version number, updating Cargo.toml / OpenAPI, `make check`, running the pre-release `ci.yml` on the Release commit, pushing the annotated tag with the Release notes as its message, and verifying the Release that GitHub Actions creates from the tag. Whenever asked for any part of cutting a version, such as "cut a release", "release v0.2.0", "tag and create the Release" or "write the Release notes", follow this skill to keep the whole consistent.
---

# Release

A Feld version is shipped by pointing a `vX.Y.Z` tag at one commit on `main` and attaching macOS and Linux archives to that tag's GitHub Release.
Users download the Release archive and place `feld` and `feld-plugin-*` in the same directory, so a Release is complete only when all three are in place: the tag, the Release notes and the archives. If you stop partway, tell the user how far you got.
This skill chooses the version number, writes the Release notes, starts the pre-release CI and pushes the tag. On the tag push, `.github/workflows/release.yml` runs the checks, builds the archives and creates the Release.

`v*` tags in this repo are never recreated. If a shipped version has a problem, or CI fails after the tag push, keep the tag and Release and ship the fixed commit from step 2 as the next patch version (skipping a version number is fine). The install script and the Homebrew formula verify archives by checksum, so if the contents of a version change, verification fails or different users get different files.

## 1. Check the preconditions

```sh
git branch --show-current      # main
git status --short             # empty
git pull --ff-only
gh auth status
gh release list --limit 3
```

Also check with `git log "$(git describe --tags --abbrev=0)"..HEAD --oneline` that there are commits since the previous tag. If there are none, there is nothing to ship, so say so and stop.

## 2. Choose the version number

Choose it from the changes since the previous tag and the previous version, following [Semantic Versioning](https://semver.org/). While on 0.x, bump minor for breaking changes and large features, and patch for fixes only.
If the user has not specified a version, propose one with a one-line rationale and confirm before continuing. A version number cannot be taken back, so do not choose it on your own.

From here on, the new version is `X.Y.Z`, its tag `vX.Y.Z`, and the previous version `P.Q.R`.

## 3. Write the version

The version number appears in two places; set both to the same value.

| File | Change |
|---|---|
| `Cargo.toml` | `version` in `[workspace.package]` |
| `docs/guide/daemon-api.openapi.yaml` | `info.version` |

The next `make check` rewrites the versions of the workspace crates (`feld`, `feld-daemon`, `feld-core`, `feld-plugin-*`) in `Cargo.lock`, so do not edit them by hand.
Run `grep -rn 'P.Q.R' README*.md docs/guide` to check that no old version remains elsewhere.

Write the Release notes in `dist/notes.md`, in words for users, from `git log vP.Q.R..HEAD` and the PRs merged in that range (`gh pr list --state merged --base main --search "merged:>=$(git log -1 --format=%cs vP.Q.R)"`). In 6 they become the tag message, and `release.yml` uses that as the Release body. The format is as follows.

- In `### Highlights` at the top, write 2-3 lines on the biggest change for users in this version. Show it to the user to confirm
- If there are breaking changes, write each one under `### Breaking` below it: what changes, followed by `Affects:` (who is affected) and `Fix:` (how to fix it). What counts follows the compatibility scope in `docs/guide/compatibility.md`. If there are none, omit the category
- Then list `### Added` / `### Changed` / `### Removed` / `### Fixed` in that order, omitting empty categories
- Start each entry with the unit changed as `**component**:`, as in `- **daemon, cli**: ...`

## 4. Verify and create the Release commit

```sh
make check
git add Cargo.toml Cargo.lock docs/guide/daemon-api.openapi.yaml
git commit -m "Release vX.Y.Z"
```

If `make check` fails, fix it before this Release commit.
`make check` rebuilds the Web UI's `web/dist/` with Vite and `web/types/feld-api.generated.d.ts` from the yaml. If either shows up in `git status --short`, the committed one is stale, so fix that before the Release commit too.

## 5. Run the pre-release CI

Before tagging, push the Release commit to the `release/vX.Y.Z` branch and start `.github/workflows/ci.yml` manually. The two configurations, minimum and latest (the table in "E2E tests and CI" in CONTRIBUTING.md), run in parallel and check against the real Claude Code and Codex as well.

```sh
git push origin HEAD:refs/heads/release/vX.Y.Z
gh workflow run ci.yml --ref release/vX.Y.Z
run=$(gh run list --workflow ci.yml --event workflow_dispatch --commit "$(git rev-parse HEAD)" --limit 1 --json databaseId --jq '.[0].databaseId')
gh run view "$run" --json url --jq .url
gh run watch "$run" --exit-status
```

Right after starting, the run may not appear in the list yet and `run` is empty; in that case wait a little and fetch it again.

Proceed to 6 only after both the `minimum` and `latest` jobs succeed. If one fails, do not tag; look at the failed step with `gh run view "$run" --log-failed` and do one of the following.

- If it is a Feld bug, fix it
- If it failed on a minimum version, raise the minimums to versions that pass in the defaults of the `workflow_dispatch` inputs in `ci.yml`, the CI table in CONTRIBUTING.md, and the "Requirements" table in the READMEs (`README.md`, `README.zh-CN.md`, `README.ja.md`)

In either case, drop the Release commit (`git reset --keep HEAD^`), commit to main, and start again from 3. When pushing the `release/vX.Y.Z` branch again, replace it with `--force`.

## 6. Create and push the tag

```sh
printf 'vX.Y.Z\n\n' | cat - dist/notes.md | git tag -a --cleanup=whitespace -F - vX.Y.Z
git push --atomic origin main vX.Y.Z
git push origin --delete release/vX.Y.Z
```

The tag is an annotated tag pointing at the Release commit on `main`, and its message body holds the Release notes written in 3. `--cleanup=whitespace` keeps the `###` headings from being stripped as comments. A pushed tag is never recreated (see the top), so before pushing, check that `git log -1 --format=%H vX.Y.Z^{commit}` and `git rev-parse main` are the same, and that this is the commit CI ran on in 5.
With `--atomic`, main and the tag are either pushed together or rejected together. If the push is rejected because `origin/main` moved during 5, delete the tag (`git tag -d vX.Y.Z`), drop the Release commit, run `git pull --ff-only`, and start again from 3.

## 7. Wait for the workflow to finish

The tag push starts `.github/workflows/release.yml`, which runs check (`cargo deny check` and `make check`), build (`package.sh` creates the macOS (`macos-15`) and Linux (`ubuntu-22.04`) archives and `.sha256` files) and release (creates the Release with the tag message body output by `notes.sh` as its body), in that order.

```sh
run=$(gh run list --workflow release.yml --event push --commit "$(git rev-parse vX.Y.Z^{commit})" --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$run" --exit-status
```

Right after the push, the run may not appear in the list yet and `run` is empty; in that case wait a little and fetch it again.

If it fails, look at the failed step with `gh run view "$run" --log-failed`. `cargo deny` reports stop check; a mismatch between the tag and the `Cargo.toml` version, or a license missing from `about.toml`, stops build.
Keep the tag and Release, and following the rule at the top, ship the fixed commit as the next patch.

## 8. Verify the Release

```sh
gh release view vX.Y.Z
dir=$(mktemp -d) && gh release download vX.Y.Z --dir "$dir"
(cd "$dir" && shasum -a 256 -c feld-vX.Y.Z-*.tar.gz.sha256 && tar -xzf feld-vX.Y.Z-aarch64-apple-darwin.tar.gz && ./feld-vX.Y.Z-aarch64-apple-darwin/feld --version)
diff <(gh release view vX.Y.Z --json body --jq .body) <(.claude/skills/release/scripts/notes.sh X.Y.Z)
```

Check that the Release has `feld-vX.Y.Z-aarch64-apple-darwin.tar.gz`, `feld-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` and their `.sha256` files attached, that the body is the tag message body (the output of `notes.sh X.Y.Z`), and that `--version` shows `X.Y.Z`.

## 9. Report

Show the Release URL, the tag, and the names and sha256 of the attached archives. As a record of the release verification, also include the URL of the `ci.yml` run from 5.
Work on the distribution side (the install script and tap) is handled outside this skill.
