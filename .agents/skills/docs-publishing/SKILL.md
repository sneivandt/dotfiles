---
name: docs-publishing
description: >
  Use for documentation organization and site management: adding or moving
  docs/content/ pages, README/site links, Astro/Starlight navigation, themes,
  search, diagrams, domain configuration, site checks, or Pages publishing.
  Not for prose-only editing (docs-writing), internal agent instructions
  (skill-maintenance), or CLI binary releases (ci-cd-patterns).
---

# Documentation Management and Publishing

Start with [site configuration](../../../docs/astro.config.mjs),
[domain declaration](../../../docs/public/CNAME), and the
[site README](../../../docs/README.md).
CI builds Astro from `docs/`; Pages deploys that verified artifact after the
aggregate succeeds for a push to `main`. Binary release selection is separate.
The root README directs readers to the published guides; keep its URLs and
heading anchors aligned with the site's output. Use
[docs-writing](../docs-writing/SKILL.md) for prose quality, factual accuracy,
examples, and the amount of detail.

## Organize the documentation

- Use the root landing page to briefly introduce the repository and give a clear
  first action linking to Getting started; keep the setup walkthrough in that
  guide rather than duplicating it on the landing page. Do not bury the first
  action beneath a link directory or oversized hero.
- Follow the [topic ownership rules](../../../docs/content/contributing.md#documentation)
  when adding or moving material. Link to the owning guide rather than creating
  another explanation of the same workflow.
- Keep everyday workflows before exhaustive reference, and source-finding
  guidance before configuration schemas. Commands and usage owns routine
  operations; CLI reference owns exact syntax and output contracts. Personal
  Git settings belong in Application settings, separate from contributor hooks.
  Keep meaningful topic groups rather than adding a generic Reference group.
- Keep Application settings focused on shared preferences and finding sources;
  Arch desktop behavior belongs in its own guide. Keep editor setup and source
  references in Application settings, agent preferences and deployment maintenance
  in Agent configuration, and recipe maintenance in Package management. Keep guides
  one level deep. Group related topics when separate headings fragment the
  navigation; a distinct page or maintenance guide does not require its own
  top-level section. Use the site configuration for current labels and order,
  not a fixed taxonomy in guidance. Split pages by distinct reader tasks,
  not length alone.
- For guides, match page title, sidebar text, filename, and URL. Update incoming
  references after moves; do not add old-name redirects or compatibility heading
  aliases. The root landing page is its own page at `/`, not a redirect to
  Getting started or a guide subject to the filename/title rule; its heading
  differs from the Dotfiles site title linked in the header.
- Keep the root README brief with the shared site icon and one prominent website
  link. Do not turn it into a second documentation directory.
- Keep site implementation, preview, checks, and publishing instructions in
  `docs/README.md`, outside the published guide collection. Published contributor
  guides may link to it instead of repeating site-maintenance procedures.

## Maintain the site

- Keep dependencies pinned in `docs/package.json` and its lockfile. Use supported
  Starlight configuration and components before adding custom JavaScript.
- Guides belong in `docs/content/`, separate from the site implementation.
  Match the front-matter title, Markdown H1, and sidebar label. Filenames and
  URLs use the lowercase, hyphenated title: `getting-started.md` publishes at
  `/getting-started.html`. Keep a description and place guides in explicit
  sidebar groups. Keep the site root at `/` as a separate landing page
  with a prominent path to the Getting started guide.
- Keep the custom content loader and Starlight's `markdown.processedDirs`
  aligned on `content/`; otherwise native callouts and heading links are skipped.
  Verify callout structure and visible styling in the rendered site.
- Keep guide links relative with `.md` extensions. The
  [Markdown transform](../../../docs/src/remark-docs.mjs) rewrites published
  destinations and preserves each Markdown title's anchor without a
  duplicate H1. Repository source links outside the guides need GitHub URLs.
- Mermaid is bundled locally through `astro-mermaid` and follows the selected
  theme. Use short labels, compact layouts, and `accTitle`/`accDescr`. Keep native
  text size and contained scrolling; do not shrink diagrams to fit mobile.
- Default new visits to dark mode while retaining explicit Light and System
  choices. Reuse Starlight's theme provider and picker, with theme credit text
  disabled. Use Starlight's default expanded sidebar groups on desktop and
  mobile; preserve its native reader toggles and saved desktop choices.
- Astro derives its site URL from `docs/public/CNAME`. Keep the homepage's
  explicit canonical/social/structured-data URLs, `public/robots.txt`, README
  links, and domain-specific browser assertions aligned when changing domains.
  The current site assumes a domain root, not a Pages project subpath.
  Pages must use **GitHub Actions**, not branch/Jekyll publishing. Forks need
  their own domain and publication setup. CNAME does not configure DNS or
  enforce HTTPS.
- Keep the landing page and guides indexable: no `noindex` metadata or robots
  exclusion on those pages; the 404 page remains nonindexable. Sitemap entries
  must match the pages' canonical URLs, including `.html` for guides, and
  `robots.txt` should point to the sitemap. Check the built output for all three.

Use the [site checks](../../../docs/README.md#checks) and
preview procedure in that README. Inspect navigation on desktop/mobile,
search, all theme choices, Mermaid SVG output, and links to source files.
The static check verifies matching titles, names, navigation, anchors, and assets;
browser tests also enforce readable diagram labels and theme preferences.
A source link check is not a rendered-site check, and
mobile emulation is not native phone coverage.

After an authorized push, follow CI and **Publish documentation** for the exact
commit, including informational results. The deployment workflow must download
the artifact from that CI run, reject pull-request, cross-repository, and
superseded runs, and avoid executing a checkout with deployment credentials.
Verify the deployed site,
HTTPS, and HTTP-to-HTTPS redirect separately. A successful build alone does not
prove deployment or browser-side rendering. DNS changes, deployment, and agent
package installation remain subject to the user's requested scope.
