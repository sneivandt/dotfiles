---
description: "Walk a fresh fork of this dotfiles repo through every rename, identity swap, and reference change needed to make it the user's own"
agent: "agent"
argument-hint: "GitHub owner/repo (e.g. alice/dotfiles), git name, and git email"
---

You are helping the user adopt this dotfiles repository as the base for their own. The repo
is currently branded for the original author (`sneivandt/dotfiles`). Rebrand it to belong to
the user, updating every hard-coded reference so the engine, CI, and self-update flow point
at *their* GitHub repository.

## Inputs to gather

If the user has not already provided them in their request, ask once for:

1. **New repo slug** in `owner/repo` form (e.g. `alice/dotfiles`). This becomes the new
   `REPO`/`$Repo` constant and the source of GitHub Releases.
2. **Git user name** for `symlinks/config/git/config`.
3. **Git email** for `symlinks/config/git/config`.
4. **Docker Hub image name** (optional). If they don't plan to publish a Docker image, tell
   them you will leave the Docker workflow disabled rather than rewriting it.

Do not invent values. If something is missing, ask.

## Required edits (must do)

These references break the self-update mechanism or misrepresent identity if left unchanged.

1. **Self-update constant** — `cli/src/phases/bootstrap/self_update.rs`
   - Replace `const REPO: &str = "sneivandt/dotfiles";` with the user's slug.
   - This is the constant the running binary uses to fetch new releases. It is *not* the
     same as the wrapper variable.

2. **Wrapper bootstrap repo** — both wrappers
   - `dotfiles.sh`: `REPO="sneivandt/dotfiles"`
   - `dotfiles.ps1`: `$Repo = "sneivandt/dotfiles"`
   - These are used for the very first binary download before self-update takes over.

3. **Tracked git identity** — `symlinks/config/git/config`
   - Update `[user] name` and `email` to the values supplied above.
   - Do *not* touch `conf/git-config.toml`; that file only holds platform-level settings
     (autocrlf, credential.helper, etc.), not user identity.

4. **Wrapper test fixture** — `.github/workflows/scripts/linux/test-shell-wrapper.sh`
   - Update the `REPO="sneivandt/dotfiles"` line so the CI shell-wrapper test exercises the
     user's own releases.

## Recommended edits (cosmetic / CI)

Update these unless the user explicitly opts out. They do not affect the engine's behaviour
but leave stale branding visible.

5. **README badges and clone URLs** — `README.md`
   - Replace `sneivandt/dotfiles` in the three CI badge URLs at the top.
   - Replace `https://github.com/sneivandt/dotfiles.git` in the two `git clone` examples
     under "Quick start".

6. **Dockerfile OCI labels** — `Dockerfile`
   - Update `org.opencontainers.image.url`, `image.source`, and `image.documentation` to
     point at the user's repo.

7. **Docker publish workflow** — `.github/workflows/docker.yml`
   - Update `images:`, `tags:`, and `repository:` references to the user's Docker Hub image
     name (gathered above). If they have no Docker Hub plans, instead recommend deleting
     `.github/workflows/docker.yml` outright and removing the Docker badge and Docker
     section from `README.md`.

8. **README Docker references** — `README.md`
   - Update the `docker run` example and the Docker Hub link under "Docker" to match the
     new image name (or remove the section if the user is dropping Docker support).

## Verification

After making the edits, run the standard checks from the repo root:

```bash
cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Then sanity-check that no stray `sneivandt` references remain (excluding their git history
and any intentional attribution they want to keep):

```bash
grep -rn "sneivandt" --exclude-dir=.git --exclude-dir=target .
```

Report any remaining hits to the user and ask whether each should be changed or kept.

## Out of scope

- Do **not** rewrite the contents of `symlinks/` (the user's own dotfiles); that is a
  manual decision they make over time.
- Do **not** touch `conf/*.toml` package lists, profiles, or manifest entries — those are
  configuration choices, not rebranding.
- Do **not** force-push, rewrite git history, or run `git commit` unless the user asks.
