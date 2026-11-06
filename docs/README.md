# Documentation site

This README is for maintaining the website at
[dotfiles.stuartneivandt.com](https://dotfiles.stuartneivandt.com/).
It stays in the repository, outside the published guide collection.
For writing or reorganizing guide content, start with
[Writing documentation](content/writing-documentation.md). Repository-wide coverage
selection lives in [Testing](content/testing.md); site-only checks are below.

## Preview locally

Use the Node version in
[.node-version](.node-version).
Use native tools for the checkout's operating system: Windows Node/npm for a
Windows checkout, Linux tools for a Linux checkout. Do not share `node_modules`
between them or run routine Windows site checks through WSL's `/mnt/c` mount.
Restore the locked dependencies with native `npm ci` after switching platforms.

The project [.npmrc](.npmrc) selects the public npm registry for documentation
dependencies, including `npm --prefix docs` commands and isolated committed
checks. It overrides user/global registry settings without changing other
projects. Command-line and environment registry overrides still take precedence.
The machine must be able to reach the registry over HTTPS; this setting does not
repair network or TLS failures.

From the repository root:

```sh
cd docs
npm ci
npm run dev
```

The server prints its local URL. To inspect production rendering and the search
index, build first, then preview:

```sh
npm run build
npm run preview
```

The commands also work from PowerShell. Neither preview publishes the site or
applies machine configuration. Inspect desktop and narrow-screen navigation,
search, theme choices, diagrams, and source links before publishing.

For a production preview restricted to this machine, build as above, then
serve only the built directory from the repository root:

```powershell
python -m http.server 4321 --bind localhost --directory docs/dist
```

Open `http://localhost:4321/` and stop the server with Ctrl+C when finished.
Use `python3` where required. The pinned VitePress preview server does not
honor `--host` as a binding restriction; its printed localhost URL does not
mean it listens only on loopback. The Python command explicitly binds loopback
and serves the public build, not the checkout or a development server.

## Checks

After installing the locked dependencies above, run this from `docs/` to
install the browser runtime:

```sh
npx playwright install chromium
```

On a minimal Linux machine, Playwright may report missing system libraries.
Its `install --with-deps chromium` command can install those libraries, but that
is a host package change: use an authorized environment or a disposable runner.
CI uses that option on its disposable Ubuntu runner.

Run the native documentation checker from the repository root:

```powershell
python docs/check.py
```

For the edit loop, prefer the affected Markdown/checker fixtures or source-link
check rather than rebuilding and rerunning browsers after every change. Reserve
one full documentation run for the committed gate unless diagnosing a failure
needs an earlier full run.

Choose the smallest check covering the edit. Commands below run from the
repository root unless the directory column says otherwise; use native tools
and `python3` where required. Node commands need the locked dependencies above.

| Edit | Directory | Focused command |
|---|---|---|
| Markdown links or destinations | Root | `python -B .github/workflows/scripts/linux/check-doc-links.py .` |
| Skill metadata or validator | Root | `python -B .agents/skills/skill-maintenance/scripts/check-skills.py .` and `python -B .agents/skills/skill-maintenance/scripts/test-check-skills.py` |
| Link, frontmatter, or rendered metadata checker | Root | `python -B docs/tests/test_content.py` |
| Native check coordinator | Root | `node --test docs/tests/check.test.mjs` |
| TypeScript or Vue | `docs/` | `npm run check` |
| Markdown rendering or Node fixtures | `docs/` | `npm test` |
| Configuration examples or JSON Lines contract | `cli/` | `cargo test --locked --profile ci --test docs_contracts` |
| APM YAML examples or selector inventory | `cli/` | `cargo test --locked --profile ci --lib documented_` |
| Keyboard, focus, or contrast | `docs/` | `npm run build` then `npm run test:browser -- --grep "keyboard|contrast"` |
| Other rendered behavior | `docs/` | `npm run build` then `npm run test:browser -- --grep "<test title>"` |

Rust commands use the pinned toolchain and isolated fixtures, not the live home
configuration. Unix-only path and symlink transitions run on native Linux;
Windows runs the portable decoder and JSON serialization cases. Review the
[documentation acceptance scenarios](content/writing-documentation.md#acceptance-scenarios)
when changing authoring guidance: metadata checks cannot prove its usefulness.

Use `python3` instead of `python` on systems that require it. The checker runs
skill metadata and its fixtures, source links, Vue/TypeScript diagnostics, Markdown and
checker fixtures, a production build, published links, and browser regressions.
It stops at the first failure. It does not install dependencies, run the Rust
compiler, or apply machine configuration.

Before pushing, commit the patch, refresh the comparison
base with `git fetch origin`, then run:

```powershell
python docs/check.py --committed
```

This compares `origin/main..HEAD` and checks documentation from the committed
code in a temporary native checkout. It restores locked npm
dependencies there and runs the committed checker, ignoring staged, unstaged,
and untracked work. Use `--base <ref>` with `--committed` for another comparison.
This also works for mixed patches, but validates only their documentation.
One successful platform is enough. For mixed patches, the coordinated
[implementation checker](content/testing.md#implementation-and-mixed-patches)
can run documentation itself, once on the native platform, alongside the
selected implementation checks. If you already ran the committed docs checker,
reuse that exact commit's result with
`--docs-checked <commit>` in the broader
implementation checker
instead of repeating site checks in WSL.

If a local npm TLS failure prevents restoring the committed dependencies,
use the coordinator's
[explicit unavailable-coverage receipt](content/testing.md#local-npm-tls-failures).
That records the gap without blocking a push or claiming documentation passed;
all other selected checks and the exact commit's CI documentation job remain required.

Rust example/serialization contracts, the task-selector fixture, and workflow
checks remain in CI. The Linux `docs` stage also runs the Rust documentation
contracts locally; the native site checker does not compile Rust. Changed
decoder or transition fixtures still need the implementation checks for their
applicable platforms.

The site checks required title/description frontmatter against rendered
metadata, navigation labels, filenames, published links, anchors, assets,
canonicals, the sitemap, and indexing metadata. Source checks resolve
current-repository GitHub `blob/main` and `tree/main` links locally, including
Markdown headings and source line ranges; unrelated external URLs are not
network-checked.
Browser regressions exercise the homepage, topic navigation,
search, theme persistence, warning callouts, and every Mermaid diagram at
desktop and mobile sizes. They also check that task details appear in the page
contents and that tables fit a narrow screen without shrinking their text.
Keyboard regressions cover skip navigation, search dismissal and restored
focus; text and safety-callout contrast is checked in both themes.
Diagram labels must stay at least 14 CSS pixels.
Mobile emulation is not a native phone/browser test.

The browser suite starts and stops its own local preview on port 4322;
keep that port free.

## Find the owning source

The site uses VitePress's default theme. Dependencies are pinned in
[package.json](package.json) and its lockfile.

The theme layout restores the previous keyboard focus after Escape closes
local search. VitePress 1.6's focus trap saves the already-focused modal input,
which disappears on dismissal. Keep the keyboard regression when upgrading;
remove this workaround when upstream restores the opener itself.

Vite and esbuild are direct, pinned development dependencies with npm overrides
so Dependabot can update them even while stable VitePress requires older
versions. Keep the overrides tied to those direct dependencies with `$vite`
and `$esbuild`; remove them when VitePress accepts patched versions itself.
The build targets Safari 14.1 or newer because patched esbuild cannot lower
destructuring for Safari 14.0. Other browser targets retain Vite 6 defaults.
Run `npm audit` after dependency changes and the full site checks before pushing.
CI audits the docs dependencies on docs changes and in the weekly dependency
security workflow. Dependabot groups security fixes separately from routine
version updates. Vite major updates stay on the tested Vite 6 line while stable
VitePress uses Rollup hooks that break with Vite 8's Rolldown bundler.
TypeScript major updates stay on hold while Vue's checker
requires the TypeScript 6 compiler API. Node type definitions stay on the major
version in `.node-version`. Revisit these restrictions when the related tools
change.

| Change | Source |
|---|---|
| Guide prose | [content/](content/) |
| Navigation, search, metadata, and output | [.vitepress/config.mts](.vitepress/config.mts) |
| Theme registration | [.vitepress/theme/index.ts](.vitepress/theme/index.ts) |
| Diagram rendering | [.vitepress/theme/MermaidDiagram.vue](.vitepress/theme/MermaidDiagram.vue) |
| Diagram containment | [.vitepress/theme/style.css](.vitepress/theme/style.css) |
| Domain and static assets | [public/](public/) |

VitePress reads only `content/` and copies assets from `public/`. Guides keep
one Markdown H1 and relative `.md` links; VitePress produces the corresponding
`.html` destinations. Heading slugs use the same GitHub-compatible algorithm as
the repository link checker. The root introduction publishes at `/`.

## Navigation and appearance

The header switches between three documentation sections. Each section has its
own sidebar and previous/next sequence:

- Guides: Introduction, Using dotfiles, Platforms, and Customization. Platform
  guides cover environment-specific setup and behavior; Using dotfiles covers
  everyday commands, profiles, configuration, safety, packages, and troubleshooting.
  Getting started leads directly to Commands and usage, then Profiles and
  categories and Configuration basics.
- Reference: direct CLI reference and Task reference overview links,
  followed by the Task details group. The overview explains task selection,
  scheduling, and removal scope; the individual pages cover each domain's
  behavior and declarations, including validation tasks. Task details are
  ordered by page title; the overview also offers a question-based lookup table.
- Development: Workflows covers contributor workflow and documentation
  authoring; Implementation covers architecture, task implementation,
  debugging, and Arch recipe maintenance;
  Verification covers running checks, writing tests, mutation testing, and CI.

Guide and Development groups start expanded with native collapse controls.
Reference has one collapsible group, Task details, rather than a separate
group for every domain. Its heading only toggles the task links. The standalone
Task reference overview stays visible above it when the group is collapsed.
Keep existing task URLs flat: the grouping is navigation, not a second set of routes.
Page boundaries follow reader tasks rather than length.
The active header link follows the page, including direct links from another
section and search results. Search covers all three sections.

The site title returns to the short personal introduction. Getting started owns
the setup walkthrough. The introduction uses the standard reading layout with
its outline and previous/next links disabled. Guides have a heading outline
through level three, source edit links, and previous/next navigation within the
current section.

Use the default theme's typography, colors, navigation, callouts, code blocks,
and local search. New visits default to dark mode; the native theme switch
retains a reader's light/dark choice. VitePress also supports following the
system preference through its saved `auto` setting.

Safety callouts use `::: warning Title` blocks. Mermaid is loaded locally on
pages containing diagrams and rerenders when the theme changes, including
during client-side navigation. Give diagrams an `accTitle` and `accDescr`,
use short labels, and retain readable text with contained horizontal scrolling.
Reference tables use VitePress's native horizontal scrolling on narrow screens.
Do not shrink tables or diagram text to force them to fit.

## Configure publishing

In **Settings > Pages > Build and deployment**, select **GitHub Actions** as
the publishing source, not **Deploy from a branch**. This is a repository
setting; editing site files does not change it.

VitePress derives its site URL from
[public/CNAME](public/CNAME),
which is also copied into the build. Keep the custom domain and HTTPS
enforcement configured in Pages. CNAME does not configure DNS or enable HTTPS.

When changing the domain, update these together:

- `public/CNAME`, which contains the hostname, not a full URL.
- The canonical and social URLs generated by [.vitepress/config.mts](.vitepress/config.mts), and the structured-data URL in
  [content/index.md](content/index.md).
- The sitemap URL in
  [public/robots.txt](public/robots.txt)
  and the published-site links in the root README.
- Domain-specific expectations in the
  [browser tests](tests/browser/site.spec.ts).

The current site assumes a domain root. Publishing a fork under a Pages project
path such as `/dotfiles/` needs a VitePress base-path configuration and a review of
root-relative links and assets; changing CNAME alone is not enough. Forks need
their own Pages and Actions setup. Publication accepts only a successful push
CI run on `main` from the same repository, not a pull-request run or a run
from another repository.

The sitemap must match page canonicals: `/` for the introduction and `.html`
URLs for guides. Guides allow indexing; the 404 page does not.

The documentation job participates in `ci-success`, including on docs-only
changes. It uploads the verified static site for a push to `main`.
The [publishing workflow](../.github/workflows/pages.yml) downloads that exact
CI run's artifact after the aggregate succeeds, rather than rebuilding it.
A completed build that no longer matches `main` is skipped rather than rolling
the site backward.

Before pushing, use the [committed-patch checker](content/testing.md#before-pushing).
After pushing, follow CI and **Publish documentation** for the exact commit.

## Diagnose publishing and indexing

| Symptom | Check |
|---|---|
| A push did not publish | The exact commit's CI and publication results; PR, cross-repository, and superseded runs do not deploy |
| The site shows old content | The deployed commit and successful Pages deployment, not only a local build |
| The custom domain fails | Pages domain settings, DNS, and HTTPS; CNAME alone is insufficient |
| A guide is not indexable | The deployed page's robots metadata and response headers, then its canonical URL and sitemap entry |
| Links or callouts changed incorrectly | Content-loader/Markdown configuration and the built output after a forced content sync |

Allowing crawlers in `robots.txt` does not override a `noindex` directive.
Correct the deployed metadata before requesting another crawl. Verify the
public pages, HTTPS, and HTTP-to-HTTPS redirect separately; a successful upload
does not prove that readers can reach the site.
