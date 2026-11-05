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
skill metadata and its fixtures, source links, Astro diagnostics, Markdown and
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

The browser suite starts and stops its own loopback preview on port 4322;
keep that port free.

## Find the owning source

The site uses Astro and Starlight with Flexoki styling. Dependencies are pinned
in [package.json](package.json)
and its lockfile.

| Change | Source |
|---|---|
| Guide prose | [content/](content/) |
| Navigation, integrations, and site defaults | [astro.config.mjs](astro.config.mjs) |
| Content loading | [src/content.config.ts](src/content.config.ts) |
| Markdown links and title anchors | [src/remark-docs.mjs](src/remark-docs.mjs) |
| Theme initialization | [src/components/ThemeProvider.astro](src/components/ThemeProvider.astro) |
| Layout and styling | [src/styles/custom.css](src/styles/custom.css) |
| Domain and static assets | [public/](public/) |

Keep the content loader and Starlight's `markdown.processedDirs` aligned on
`content/`; otherwise native callouts and heading links are not processed.
The Markdown transform removes the duplicate displayed H1, preserves its
anchor, and converts relative `.md` links to published URLs.

After changing that processing pipeline, run `npm exec -- astro sync --force`
from `docs/` before rebuilding so unchanged guides receive the new transforms.

## Navigation and appearance

Keep guides one level deep in the sidebar. Put maintenance sections after
everyday guidance in their owning guide. Group related topics under recognizable
headings rather than giving every topic its own small section. Page boundaries follow
reader tasks; they do not need to match navigation groups.

The site title links to the separate introduction at `/`; Getting started
owns the setup walkthrough.

Keep the compact spacing between the header icon and title. Use the template's
navigation and scrollbar appearance without custom overrides. Navigation groups start expanded on desktop and mobile, with
native reader toggles preserved. New visits default to dark mode, while the theme
control retains explicit Light and System choices.

Keep source and topic links visually distinct from literal syntax: ordinary
link labels should not also be boxed as inline code. Use
[Docs writing](../.agents/skills/docs-writing/SKILL.md#choose-links-code-and-emphasis)
for link, code, and bold-emphasis choices; do not compensate for excessive
markup by hiding code styling in the theme.

Prefer Starlight's built-in callouts, navigation, search, and theme controls to
custom components. Mermaid is bundled locally and follows the selected theme.
Give diagrams an `accTitle` and `accDescr`, use short labels, and retain readable
text size with contained scrolling rather than shrinking diagrams to fit.

## Configure publishing

In **Settings > Pages > Build and deployment**, select **GitHub Actions** as
the publishing source, not **Deploy from a branch**. This is a repository
setting; editing site files does not change it.

Astro derives its site URL from
[public/CNAME](public/CNAME),
which is also copied into the build. Keep the custom domain and HTTPS
enforcement configured in Pages. CNAME does not configure DNS or enable HTTPS.

When changing the domain, update these together:

- `public/CNAME`, which contains the hostname, not a full URL.
- The homepage's explicit canonical, social, and structured-data URLs in
  [content/index.md](content/index.md).
- The sitemap URL in
  [public/robots.txt](public/robots.txt)
  and the published-site links in the root README.
- Domain-specific expectations in the
  [browser tests](tests/browser/site.spec.ts).

The current site assumes a domain root. Publishing a fork under a Pages project
path such as `/dotfiles/` needs an Astro base-path configuration and a review of
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
