---
name: docs-publishing
description: >
  Use when writing or reorganizing README.md or docs/content/ guides, documenting
  changed user behavior, or changing the Astro/Starlight site's navigation, themes,
  search, diagrams, domain, or Pages publishing. Owns human-readable documentation
  and site consistency. Not for internal agent instructions (skill-maintenance)
  or CLI binary release implementation (ci-cd-patterns).
---

# Documentation Quality and Publishing

Start with [site configuration](../../../docs/astro.config.mjs),
[domain declaration](../../../docs/public/CNAME), and the
[publishing guide](../../../docs/content/contributing.md#published-documentation).
CI builds Astro from `docs/`; Pages deploys that verified artifact after the
aggregate succeeds for a push to `main`. Binary release selection is separate.
The root README directs readers to the published guides; keep its URLs and
heading anchors aligned with the site's output.

## Write for readers

- Use the root landing page to briefly introduce the repository and give a clear
  first action linking to Getting started; keep the setup walkthrough in that
  guide rather than duplicating it on the landing page. Do not bury the first
  action beneath a link directory or oversized hero. In guides, put the common
  workflow before inventories, internals, and exceptions; explain terms on
  first use.
- Prefer short, copyable commands using verified defaults. Avoid routine
  `--root .`, profile overrides, and diagnostic flags. Keep flags that express
  the example's purpose, such as a targeted preview or explicit unattended input.
  Explain side effects where they matter, without repeating every caveat everywhere.
- Give each explanation one home. Commands and logs belong in Commands and usage;
  the end-to-end update story in Update lifecycle; formats and private additions
  under Customize; platform/editor/package workflows in Subsystem guides;
  implementation and validation under Development. Link instead of duplicating.
- Keep Application settings focused on shared preferences and finding sources;
  detailed Arch desktop and editor behavior belongs in its owning guide. Put
  advanced recipe maintenance after everyday package use. Keep related platforms
  and editors adjacent in the sidebar, and order groups from setup to development.
- For guides, match page title, sidebar text, filename, and URL. Update incoming
  references after moves; do not add old-name redirects or compatibility heading
  aliases. The root landing page is its own page at `/`, not a redirect to
  Getting started or a guide subject to the filename/title rule; its heading
  differs from the Dotfiles site title linked in the header.
- Keep the README brief with the shared site icon and one prominent root website
  link. Do not turn it into a second documentation directory.
- Verify behavioral claims against the owning code/tests, not just existing prose.
  Use public synthetic examples, never private overlays or unsanitized logs.

## Maintain the site

- Keep dependencies pinned in `docs/package.json` and its lockfile. Use supported
  Starlight configuration and components before adding custom JavaScript.
- Guides belong in `docs/content/`, separate from the site implementation.
  Match the front-matter title, Markdown H1, and sidebar label. Filenames and
  URLs use the lowercase, hyphenated title: `getting-started.md` publishes at
  `/getting-started.html`. Keep a description and place guides in the explicit,
  expanded sidebar groups. Keep the site root at `/` as a separate landing page
  with a prominent path to the Getting started guide.
- Keep guide links relative with `.md` extensions. The
  [Markdown transform](../../../docs/src/remark-docs.mjs) rewrites published
  destinations and preserves each Markdown title's anchor without a
  duplicate H1. Repository source links outside the guides need GitHub URLs.
- Mermaid is bundled locally through `astro-mermaid` and follows the selected
  theme. Use short labels, compact layouts, and `accTitle`/`accDescr`. Keep native
  text size and contained scrolling; do not shrink diagrams to fit mobile.
- Default new visits to dark mode while retaining explicit Light and System
  choices. Reuse Starlight's theme provider and picker; keep groups expanded and
  theme credit text disabled.
- The canonical site URL comes from `docs/public/CNAME`. Pages must use
  **GitHub Actions**, not branch/Jekyll publishing. Forks need their own domain
  configuration. CNAME does not configure DNS or enforce HTTPS.
- Keep the landing page and guides indexable: no `noindex` metadata or robots
  exclusion on those pages; the 404 page remains nonindexable. Sitemap entries
  must match the pages' canonical URLs, including `.html` for guides, and
  `robots.txt` should point to the sitemap. Check the built output for all three.

Use the [site checks](../../../docs/content/testing.md#documentation-site) and
preview procedure in the publishing guide. Inspect navigation on desktop/mobile,
search, all theme choices, Mermaid SVG output, and links to source files.
The static check verifies matching titles, names, navigation, anchors, and assets;
browser tests also enforce readable diagram labels and theme preferences.
A source link check is not a rendered-site check, and
mobile emulation is not native phone coverage.

After an authorized push, follow CI and **Publish documentation** for the exact
commit, including informational results. The deployment workflow must download
the artifact from that CI run, reject PR/fork/superseded runs, and avoid executing
a checkout with deployment credentials. Verify the deployed site,
HTTPS, and HTTP-to-HTTPS redirect separately. A successful build alone does not
prove deployment or browser-side rendering. DNS changes, deployment, and agent
package installation remain subject to the user's requested scope.
