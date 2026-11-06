# Documentation site

This README is for maintaining the website at
[dotfiles.stuartneivandt.com](https://dotfiles.stuartneivandt.com/).
It stays in the repository, outside the published guide collection.
For writing or reorganizing guide content, start with
[Contributing](content/contributing.md#documentation). Repository-wide coverage
selection lives in [Testing](content/testing.md); site-only checks are below.

## Preview locally

Use the Node version in
[.node-version](.node-version).
Use native tools for the checkout's operating system: Windows Node/npm for a
Windows checkout, Linux tools for a Linux checkout. Do not share `node_modules`
between them or run routine Windows site checks through WSL's `/mnt/c` mount.
Restore the locked dependencies with native `npm ci` after switching platforms.

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

The Rust task-selector fixture and workflow checks remain in CI. The Linux
`docs` stage also runs the selector fixture locally; it is not required just to
validate a documentation patch on Windows.

The site checks matching titles, navigation labels, filenames, published links,
anchors, assets, canonicals, the sitemap, and indexing metadata.
Browser regressions exercise the homepage, topic navigation,
search, theme persistence, warning callouts, and every Mermaid diagram at
desktop and mobile sizes. They also check that task details appear in the page
contents and that tables fit a narrow screen without shrinking their text.
Diagram labels must stay at least 14 CSS pixels.
Mobile emulation is not a native phone/browser test.

The browser suite starts and stops its own local preview on port 4322;
keep that port free.

## Find the owning source

The site uses VitePress's default theme. Dependencies are pinned in
[package.json](package.json) and its lockfile.

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

- Guides: Introduction, Workflows, Platforms, and Customization. Platform
  guides cover environment-specific setup and behavior; Workflows covers
  everyday commands, packages, and troubleshooting.
- Reference: CLI syntax and shared configuration rules, followed by task pages
  grouped into checkout and shell, files and Windows, packages and services,
  agents and overlays, and validation. Each task page combines its behavior and
  configuration.
- Development: contributor workflow, testing, architecture, and CI/releases.

Keep sidebar groups one level deep, starting expanded and collapsible with
the native arrow controls. Reference uses groups to keep its task pages easy
to scan; Development remains a short direct list. Page boundaries follow reader
tasks rather than length.
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
