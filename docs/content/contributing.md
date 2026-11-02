---
title: Contributing
description: Find the right place for a change, set up the pinned toolchain, and follow the contribution and verification workflow.
---

# Contributing

Most changes start with one application's files, not the CLI engine. This guide
helps you find the right place, make a bounded change, and verify what you will
push. Use [Architecture](architecture.md) for design details and
[Testing](testing.md) for exact checks.

## Development setup

You need Git, rustup-managed Cargo/Rust, and native build tools for the CLI's
C dependencies. On Arch, provide `base-devel`, `pkgconf`, and `openssl`.
For Windows's default MSVC toolchain, install the C++ build tools and Windows
SDK requested by the Rust installer.

Rustup selects the repository's pinned toolchain when you enter `cli/`.
From the checkout:

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
[local stages](testing.md#fast-local-sequence) explain their requirements.

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
| Packages, links, settings, or services | The matching file under [`conf/`](https://github.com/sneivandt/dotfiles/tree/main/conf/); see [Configuration](configuration-reference.md) |
| Administrator-owned file content | [`system/`](https://github.com/sneivandt/dotfiles/tree/main/system/) and [`conf/system-files.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/system-files.toml) |
| CLI options or command selection | [`app/cli.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/cli.rs) and [`app/commands/`](https://github.com/sneivandt/dotfiles/tree/main/cli/src/app/commands/) |
| Task membership or cross-domain dependencies | [`app/catalog.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs) |
| How one kind of state is applied | Its module under [`domains/`](https://github.com/sneivandt/dotfiles/tree/main/cli/src/domains/) |
| Process, filesystem, or output mechanics | [`infra/`](https://github.com/sneivandt/dotfiles/tree/main/cli/src/infra/) |
| Hooks or automation | [Hooks](git-hooks.md) and [CI and releases](ci-and-releases.md) |

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

Start with [Choosing coverage](testing.md#choosing-coverage). Before committing,
review the diff for unintended behavior, private data, generated files, and
stale examples. Stage explicit paths or hunks and inspect `git diff --cached`.
The [commit hooks](git-hooks.md) inspect the index, which may differ from your
working files.

After committing, run the [pre-push checker](testing.md#before-pushing).
It verifies the committed range, not unstaged fixes. Resolve local failures and
record unavailable checks. After pushing, follow the exact commit's
[CI result](ci-and-releases.md#reading-a-run), including informational failures.

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
   in [Configuration](configuration-reference.md).

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
[task reference](task-reference.md). [Architecture](architecture.md) explains the contracts
and existing task helpers.

### Platform-specific changes

Use runtime capabilities and injected adapters for decisions. Keep compile-time
gates around platform-only APIs so the other build still compiles. Tests should
not change process-global state or invoke real package managers or elevation.

Cross-target compilation is not a Windows runtime test. See
[Platform coverage parity](testing.md#platform-coverage-parity).

## Documentation

Write for the person doing the work. Put common tasks before edge cases, explain
terms when introducing them, and keep examples short, copyable, and safe in
context. Prefer verified defaults over routine root/profile overrides and
diagnostic flags. Keep necessary task filters and preview flags. Use a small
Mermaid diagram only when it makes a relationship easier to grasp.

The standalone [introduction](index.md) gives readers a short path to
[Getting started](getting-started.md), everyday use, and customization.
The sidebar moves from setup to everyday use, customization,
subsystem guides, and development. Keep each subsystem's everyday and maintenance
guides together: AI agents and Packages each have a subgroup under Subsystem
guides. Keep related platforms and editors adjacent. Reserve Development for
repository-wide contribution and implementation topics.

Give each explanation one home: commands and logs in
[Commands and usage](commands-and-usage.md), the update story in
[Update lifecycle](update-lifecycle.md), formats and private additions under
Customize, and detailed platform/editor/package workflows in Subsystem guides.
Link instead of repeating the same instructions. Split pages when their readers
or tasks genuinely differ; update incoming references when moving content.
For example, [AI agent configuration](ai-agent-configuration.md) covers adding
content, while [Agent deployment internals](agent-deployment-internals.md)
covers maintaining the deployment implementation. Everyday
[Package management](package-management.md) is separate from
[Local package recipes](local-package-recipes.md).

Link a tracked file at the point where readers need to inspect or edit it,
usually its first actionable mention or an owning-source table. Use code
styling without a link for installed/generated paths, example placeholders,
and repeated filenames. Do not link a hypothetical file as if it already
exists. In guides, tracked source links use full GitHub URLs; links to other
guides stay relative with `.md` extensions.

Keep the root README a landing page with the shared icon and one prominent
website link. Run the documentation check after changing
links, headings, filenames, or selectors. It catches link and catalog drift,
not incorrect prose: verify behavioral claims against code and tests.

Never use private overlays, credentials, or unsanitized logs as public examples.

### Published documentation

The repository [Docs publishing skill](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/docs-publishing/SKILL.md)
covers readable guide prose, examples, navigation, and site publishing for agents.

The documentation home is [dotfiles.stuartneivandt.com](https://dotfiles.stuartneivandt.com/).
The site uses Astro and Starlight with Flexoki theme styling, with locked
dependencies in `docs/package-lock.json`.
Markdown guides live in `docs/content/`; site configuration, components, and
static assets are separate. The same guides remain readable in the repository.

Use the same wording for each guide's front-matter `title`, Markdown H1, and
sidebar label. Its filename and URL use the lowercase, hyphenated title:
`getting-started.md` publishes at `/getting-started.html`. The exception is
`index.md`, which builds `index.html` and is served at `/`; its page heading is
separate from the Dotfiles site title linked in the header and it does not
appear in the sidebar. Add a `description` and place new guides in the
appropriate sidebar group in `docs/astro.config.mjs`.
All groups use Starlight's default expanded navigation on desktop and mobile.
Readers can collapse groups themselves. New visits default to dark mode; the
native theme control retains an explicit Light or System choice.

The site root is a short introduction, separate from Getting started. The
header title links back to it on desktop and mobile. Renamed pages do not get
compatibility routes or old-heading aliases: update their incoming references.
The Markdown transform avoids a duplicate displayed H1, retains the current
title's Markdown fragment, and converts relative `.md` links to published URLs.
Use full GitHub URLs for source files outside the guides.

Use Starlight's built-in callouts, navigation, search, and theme controls before
adding custom components. Mermaid is bundled locally and follows the reader's
theme. Give each diagram an `accTitle` and `accDescr`, use short labels, and
prefer compact top-to-bottom layouts. Diagrams keep their native text size;
the container scrolls if a figure cannot fit. Do not scale labels into illegibility.

From the repository root, with the Node version selected by `docs/.node-version`:

```sh
cd docs
npm ci
npx playwright install chromium
npm run dev
```

The dev server prints its local URL. For the production rendering and search
index, build first, then preview:

```sh
npm run build
npm run preview
```

The same commands work from PowerShell. Inspect desktop and narrow-screen
navigation, search, light/dark/system themes, diagrams, and source links.
The automated [documentation checks](testing.md#documentation-site) cover these
contracts without changing the host's managed configuration.

GitHub Pages must use **GitHub Actions** as its publishing source, not
**Deploy from a branch**. For the initial migration, change this in the
repository's **Settings > Pages > Build and deployment** before publishing.
This is a repository setting; editing site files does not switch it automatically.
Keep the custom domain and HTTPS enforcement enabled. The canonical site URL
is derived from `docs/public/CNAME`, which is also copied into the build.
`docs/public/robots.txt` points crawlers to the sitemap; its domain must also
change in a fork. The sitemap lists the same `.html` URLs used as page
canonicals. Guides allow indexing, while the 404 page opts out. Forks must
configure their own domain or Pages URL.

CI builds and checks the site, including Chromium desktop/mobile regressions.
After the aggregate succeeds for a same-repository push to `main`, the
**Publish documentation** workflow deploys the artifact from that exact CI run.
It does not rebuild or execute repository code with deployment credentials,
and it skips builds superseded on `main`.

Before pushing, run the [local docs stage](testing.md#fast-local-sequence).
For a link-only check from the repository root:

```sh
python3 -B .github/workflows/scripts/linux/check-doc-links.py .
```

After an authorized push, follow both CI and **Publish documentation** for the
exact commit. Verify the deployed pages, custom domain, HTTPS, and HTTP-to-HTTPS
redirect separately; a successful upload does not prove the public site works.
If a search console reports a `noindex` directive, check the deployed page's
robots metadata and response headers before requesting a fresh crawl. Allowing
crawlers in `robots.txt` does not override a `noindex` directive.
