---
title: Documentation site
description: Preview and maintain the Astro and Starlight site, configure its domain, and diagnose publishing or indexing problems.
---

# Documentation site

This guide is for maintaining the website at
[dotfiles.stuartneivandt.com](https://dotfiles.stuartneivandt.com/).
For writing or reorganizing guide content, start with
[Contributing](contributing.md#documentation). Automated checks and their
prerequisites live in [Testing](testing.md#documentation-site).

## Preview locally

Use the Node version in
[`docs/.node-version`](https://github.com/sneivandt/dotfiles/blob/main/docs/.node-version).
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

## Find the owning source

The site uses Astro and Starlight with Flexoki styling. Dependencies are pinned
in [`docs/package.json`](https://github.com/sneivandt/dotfiles/blob/main/docs/package.json)
and its lockfile.

| Change | Source |
|---|---|
| Guide prose | [`docs/content/`](https://github.com/sneivandt/dotfiles/tree/main/docs/content/) |
| Navigation, integrations, and site defaults | [`astro.config.mjs`](https://github.com/sneivandt/dotfiles/blob/main/docs/astro.config.mjs) |
| Content loading | [`src/content.config.ts`](https://github.com/sneivandt/dotfiles/blob/main/docs/src/content.config.ts) |
| Markdown links and title anchors | [`src/remark-docs.mjs`](https://github.com/sneivandt/dotfiles/blob/main/docs/src/remark-docs.mjs) |
| Theme initialization | [`src/components/ThemeProvider.astro`](https://github.com/sneivandt/dotfiles/blob/main/docs/src/components/ThemeProvider.astro) |
| Layout and styling | [`src/styles/custom.css`](https://github.com/sneivandt/dotfiles/blob/main/docs/src/styles/custom.css) |
| Domain and static assets | [`public/`](https://github.com/sneivandt/dotfiles/tree/main/docs/public/) |

Keep the content loader and Starlight's `markdown.processedDirs` aligned on
`content/`; otherwise native callouts and heading links are not processed.
The Markdown transform removes the duplicate displayed H1, preserves its
anchor, and converts relative `.md` links to published URLs.

After changing that processing pipeline, run `npm exec -- astro sync --force`
from `docs/` before rebuilding so unchanged guides receive the new transforms.

## Navigation and appearance

Keep guides one level deep in the sidebar, with everyday and maintenance
guides grouped by topic. The site title links to the separate introduction at
`/`; Getting started owns the setup walkthrough.

Navigation groups start expanded on desktop and mobile, with native reader
toggles preserved. Overflowing navigation uses a full-width, contrasting
scrollbar in both themes. New visits default to dark mode, while the theme
control retains explicit Light and System choices.

Prefer Starlight's built-in callouts, navigation, search, and theme controls to
custom components. Mermaid is bundled locally and follows the selected theme.
Give diagrams an `accTitle` and `accDescr`, use short labels, and retain readable
text size with contained scrolling rather than shrinking diagrams to fit.

## Configure publishing

In **Settings > Pages > Build and deployment**, select **GitHub Actions** as
the publishing source, not **Deploy from a branch**. This is a repository
setting; editing site files does not change it.

The canonical site URL comes from
[`public/CNAME`](https://github.com/sneivandt/dotfiles/blob/main/docs/public/CNAME),
which is also copied into the build. Keep the custom domain and HTTPS
enforcement configured in Pages. CNAME does not configure DNS or enable HTTPS.
Forks must use their own domain or Pages URL and update the sitemap URL in
[`public/robots.txt`](https://github.com/sneivandt/dotfiles/blob/main/docs/public/robots.txt).

The sitemap must match page canonicals: `/` for the introduction and `.html`
URLs for guides. Guides allow indexing; the 404 page does not.

Before pushing, use the [committed-patch checker](testing.md#before-pushing).
After pushing, follow CI and **Publish documentation** for the exact commit.
The [publishing workflow](ci-and-releases.md#documentation-publishing) deploys
the verified CI artifact rather than rebuilding it.

## Diagnose publishing and indexing

| Symptom | Check |
|---|---|
| A push did not publish | The exact commit's CI and publication results; PR, fork, and superseded runs do not deploy |
| The site shows old content | The deployed commit and successful Pages deployment, not only a local build |
| The custom domain fails | Pages domain settings, DNS, and HTTPS; CNAME alone is insufficient |
| A guide is not indexable | The deployed page's robots metadata and response headers, then its canonical URL and sitemap entry |
| Links or callouts changed incorrectly | Content-loader/Markdown configuration and the built output after a forced content sync |

Allowing crawlers in `robots.txt` does not override a `noindex` directive.
Correct the deployed metadata before requesting another crawl. Verify the
public pages, HTTPS, and HTTP-to-HTTPS redirect separately; a successful upload
does not prove that readers can reach the site.
