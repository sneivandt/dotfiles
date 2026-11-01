---
title: Contributing
nav_order: 13
---

# Contributing

Most changes start with one application's files, not the CLI engine. This guide
helps you find the right place, make a bounded change, and verify what you will
push. Use [Architecture](ARCHITECTURE.md) for design details and
[Testing](TESTING.md) for exact checks.

## Development setup

You need Git and the repository's pinned Rust toolchain. From the checkout:

```sh
cd cli
cargo build --locked --profile ci
cargo run --locked --profile ci -- --help
```

This runs current source. Do not use a bootstrap wrapper to test Rust changes:
it may download a release or reuse a different binary.

### Selecting the pinned toolchain

Use rustup-managed `cargo` and `rustc`, and run them from `cli/`. Rustup reads
[`rust-toolchain.toml`](https://github.com/sneivandt/dotfiles/blob/main/cli/rust-toolchain.toml) there and can install missing
components or targets.

```sh
cd cli
rustup show active-toolchain
cargo --version
rustc --version
```

Check the reported toolchain against the file before validation. System-installed
Rust tools can ignore it. Configure your editor to use the same toolchain and
restart its Rust language server after switching.

The minimum supported Rust version in [`Cargo.toml`](https://github.com/sneivandt/dotfiles/blob/main/cli/Cargo.toml) is a
separate compatibility check, not the compiler to use for ordinary development.
If a required compiler or component is unavailable, report the missing coverage
rather than substituting another version and calling it a pass.

### Builds and debugging

Use `cargo build` and `cargo test` for a normal edit/build loop, or the `ci`
profile to reproduce verification. Local builds keep incremental compilation,
debug assertions, and overflow checks, but omit debug information to reduce
build time and disk use.

To restore debugger information for a Linux build:

```sh
CARGO_PROFILE_DEV_DEBUG=2 cargo build --locked
```

Use `line-tables-only` instead of `2` for file/line backtraces, or
`CARGO_PROFILE_TEST_DEBUG` for tests. In PowerShell, set the corresponding
environment variable before invoking Cargo. Changing these settings rebuilds
affected artifacts.

Install optional analyzers only for the checks you need. The
[local stages](TESTING.md#fast-local-sequence) explain their requirements.

## Contribution workflow

### 1. Check the starting point

```sh
git status --short --branch
git diff --stat
git diff --cached --stat
```

Preserve unrelated work. This checkout may be the live source of home-directory
symlinks: editing `symlinks/` can affect an application immediately. Edit tracked
sources, not installed copies.

Use isolated fixtures, not a real `update` or `remove`, to test changes. Those
commands can alter home files, packages, Git settings, the registry, and system
configuration. A clean working tree does not make a live installation safe.

### 2. Find the owning files

| What is changing? | Start here |
|---|---|
| Application configuration | The application's directory under [`symlinks/`](https://github.com/sneivandt/dotfiles/tree/main/symlinks/) |
| Packages, links, settings, or services | The matching file under [`conf/`](https://github.com/sneivandt/dotfiles/tree/main/conf/); see [Configuration](CONFIGURATION.md) |
| Administrator-owned file content | [`system/`](https://github.com/sneivandt/dotfiles/tree/main/system/) and [`conf/system-files.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml) |
| CLI options or command selection | [`app/cli.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/cli.rs) and [`app/commands/`](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/commands/) |
| Task membership or cross-domain dependencies | [`app/catalog.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs) |
| How one kind of state is applied | Its module under [`domains/`](https://github.com/sneivandt/dotfiles/tree/main/cli/src/domains/) |
| Process, filesystem, or output mechanics | [`infra/`](https://github.com/sneivandt/dotfiles/tree/main/cli/src/infra/) |
| Hooks or automation | [Hooks](HOOKS.md) and [CI and releases](CI.md) |

Read the closest existing implementation before adding a new abstraction.
Repository [skills](https://github.com/sneivandt/dotfiles/tree/main/.agents/skills/) contain focused contributor procedures;
they do not replace the source or these guides. For common configuration work,
start with [shell configuration](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/shell-configuration/SKILL.md),
[Hyprland sessions](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/hyprland-session/SKILL.md),
[Quickshell](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/desktop-shell/SKILL.md), or
[Vim/Neovim](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/editor-configuration/SKILL.md).
[Self-update](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/self-update/SKILL.md) covers binary replacement;
[Rust patterns](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/rust-patterns/SKILL.md) routes unfamiliar CLI work.

### 3. Change the behavior and its proof

Keep desired-state edits declarative. For a behavior change, include the
configuration, implementation, wiring, regression test, and relevant docs.
Avoid unrelated cleanup that obscures the fix.

Test the observable failure, not just the happy path. State-changing code should
cover repeat execution, preview, unavailable prerequisites, and partial failure
where relevant. Test removal only where the feature promises it: `remove` is
not a universal undo operation.

### 4. Review, commit, and verify the push

Start with [Choosing coverage](TESTING.md#choosing-coverage). Before committing,
review the diff for unintended behavior, private data, generated files, and
stale examples. Stage explicit paths or hunks and inspect `git diff --cached`.
The [commit hooks](HOOKS.md) inspect the index, which may differ from your
working files.

After committing, run the [pre-push checker](TESTING.md#before-pushing).
It verifies the committed range, not unstaged fixes. Resolve local failures and
record unavailable checks. After pushing, follow the exact commit's
[CI result](CI.md#reading-a-run), including informational failures.

## Change checklists

### Config-backed state

For an existing field or category, change the data and its focused regression.
For a new configuration section:

1. Add its typed decoder and semantic validation in the owning domain, including
   invalid-input tests.
2. Add it to `config_section_inventory!` in
   [`app/config`](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/config/). This provides its field, typed store
   handle, summary count, and empty unit fixture.
3. Wire any special loading, overlay behavior, provenance, and platform
   filtering in `Config::load`, and validation in `Config::validate`.
4. Implement the resource or operation, register its task, and add real
   configuration plus command/configuration tests.
5. Keep conditional symlinks aligned with categories and document the format
   in [Configuration](CONFIGURATION.md).

### Tasks and workflows

A **resource** manages independent items; an **operation** manages a workflow
that converges as one unit. Reuse their lifecycle helpers rather than writing
another preview/apply/result-accounting loop.

Give a task a stable selector and explicit applicability, prerequisites,
privilege requirements, and command membership. Static update/remove tasks go
in the catalog; command-specific tasks belong to that command. Same-domain
dependencies stay in the domain, and cross-domain dependencies belong in the
app layer. List order does not schedule work.

Use a failure-blocking dependency when success is required, and an ordering-only
dependency when the task should wait and then recheck state itself. Preserve
selection, `--with-deps`, and discovery behavior; update the
[task reference](TASKS.md). [Architecture](ARCHITECTURE.md) explains the contracts
and existing task helpers.

### Platform-specific changes

Use runtime capabilities and injected adapters for decisions. Keep compile-time
gates around platform-only APIs so the other build still compiles. Tests should
not change process-global state or invoke real package managers or elevation.

Cross-target compilation is not a Windows runtime test. See
[Platform coverage parity](TESTING.md#platform-coverage-parity).

## Documentation

Write for the person doing the work. Put common tasks before edge cases, explain
terms when introducing them, and keep examples copyable and safe in context.
Use a small Mermaid diagram only when it makes a relationship easier to grasp.

The [documentation index](README.md) is the reader's map. Put user behavior in
Usage, desired-state formats in Configuration, design in Architecture, and
verification in Testing. Link to one canonical explanation instead of repeating
it in several guides. Split a long page when its readers or tasks genuinely
differ; preserve incoming links when moving content.

Keep the root README a landing page. Run the documentation check after changing
links, headings, filenames, or selectors. It catches link and catalog drift,
not incorrect prose: verify behavioral claims against code and tests.

Never use private overlays, credentials, or unsanitized logs as public examples.

### Published documentation

The repository [Docs publishing skill](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/docs-publishing/SKILL.md)
covers site configuration and deployment checks for agents.

The documentation home is [dotfiles.stuartneivandt.com](https://dotfiles.stuartneivandt.com/).
GitHub Pages publishes `docs/` from `main` using Jekyll. The pinned Just the Docs
theme in `docs/_config.yml` provides the dark color scheme, sidebar, mobile menu,
search, and Mermaid rendering; no separate copy of the guides is maintained.
`docs/_includes/mermaid_config.js` gives diagrams a matching dark theme.

Edit the existing Markdown files. Each guide has YAML front matter with its
navigation `title` and numeric `nav_order`; `docs/README.md` has `permalink: /`
and remains the home page. Give new guides a unique navigation order and add
them to the documentation index. Keep links between guides relative, including
the `.md` extension and any heading anchor: Jekyll converts those destinations
to the published pages. Link to repository source files with full GitHub URLs,
since code and configuration outside `docs/` are not published to the site.

Keep `docs/CNAME` set to `dotfiles.stuartneivandt.com` and the site's `url` in
`docs/_config.yml` aligned with it. Forks should configure their own Pages URL
and custom domain before publishing.

Before pushing, run the documentation link check from the repository root:

```sh
python3 -B .github/workflows/scripts/linux/check-doc-links.py .
```

For a site preview, use a Ruby environment with the `github-pages` gem and run
`jekyll serve --source docs --destination /tmp/dotfiles-docs-site`. Check the
sidebar and mobile menu, search, diagrams, and links to repository files. Restart
the preview after changing `_config.yml`.

After an authorized push to `main`, inspect the **pages build and deployment**
run in GitHub Actions. It is separate from the CLI CI/release workflows. Confirm the deploy job succeeds, then check the published pages for rendering
and navigation problems.
