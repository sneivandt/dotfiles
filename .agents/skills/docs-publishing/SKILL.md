---
name: docs-publishing
description: >
  Use for documentation organization and site management: adding or moving
  docs/content/ pages, README/site links, VitePress navigation, themes,
  search, diagrams, domain configuration, site checks, or Pages publishing.
  Not for prose-only editing (docs-writing), internal agent instructions
  (skill-maintenance), or CLI binary releases (ci-cd-patterns).
---

# Documentation Management and Publishing

Start with [site configuration](../../../docs/.vitepress/config.mts),
[domain declaration](../../../docs/public/CNAME), and the
[site README](../../../docs/README.md).
CI builds VitePress from `docs/`; Pages deploys that verified artifact after the
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
- Follow the [topic ownership rules](../../../docs/content/writing-documentation.md#choose-the-owning-page)
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

- Run documentation checks with the checkout's native toolchain. For a Windows
  checkout, use Windows Python, Node/npm, and Playwright, not WSL against
  `/mnt/c`. Restore dependencies for that OS; do not share `node_modules`
  between Windows and Linux. Resolve missing tools or registry failures without
  bypassing TLS or moving the whole site check to another OS. One platform is
  sufficient, including mixed patches; do not repeat successful docs checks
  in WSL for cross-platform CLI coverage.
- Keep dependencies pinned in `docs/package.json` and its lockfile. Extend
  VitePress's default theme only where the documentation needs behavior it does
  not supply; prefer its native navigation, callouts, search, and code blocks.
- Guides belong in `docs/content/`, selected by `srcDir`. Match the front-matter
  title, Markdown H1, and sidebar label. Filenames and URLs use lowercase,
  hyphenated titles; `getting-started.md` publishes at `/getting-started.html`.
  Keep a description and an explicit sidebar entry for every guide.
- Keep one Markdown H1 and relative `.md` guide links. VitePress handles the
  published link extensions; the configured GitHub-compatible heading slugger
  preserves repository anchors. Repository source links need GitHub URLs.
- Keep the root introduction short, with a prominent Getting started link.
  Use the standard reading layout with outline and previous/next links disabled.
  Put platform setup and behavior in Platforms, package workflows in Using dotfiles,
  and exact CLI/task lookups in Reference. Configuration basics belongs in
  Guides after Profiles and categories; task pages own declaration formats.
  Preserve native mobile navigation. Header links switch between
  Guides, Reference, and Development, each with its own sidebar and previous/next
  sequence. Guides has Introduction, Using dotfiles, Platforms, and Customization.
  Its groups start expanded with native right-hand collapse arrows. Reference
  has direct CLI reference and Task reference overview links above one unlinked,
  collapsible Task details group, not separate domain groups. Keep the
  overview outside the group so collapsing it never hides that link.
  Keep task URLs flat and include nested children when deriving section matching
  and sidebar route coverage. Development groups contributor workflows,
  implementation/debugging guidance, and verification. Testing owns coverage
  selection and execution commands; Writing tests owns regression design,
  fixture construction, and assertions. Writing documentation is the human
  authoring guide; do not substitute skill links in published prose. Use
  [task-reference-writing](../task-reference-writing/SKILL.md) for task-page
  content and evidence requirements. Keep the catalog an index, not a second
  copy of each task's explanation.
- Use `::: warning Title` for safety callouts. Verify their title, body, and
  visible styling in the rendered site. The guide outline includes levels 2–3.
- Mermaid is rendered by the local theme component and follows theme changes
  and client-side navigation. Use `accTitle`/`accDescr`, short labels, and native
  text size with contained horizontal scrolling. Tables also scroll within
  their content area on narrow screens rather than shrinking text.
- New visits default to dark mode; preserve the native appearance switch and
  reader preferences. Use the default theme's colors, typography, and spacing.
- VitePress derives its site URL from `docs/public/CNAME`. Keep generated canonical/social URLs and the homepage's
  explicit structured-data URL, `public/robots.txt`, README
  links, and domain-specific browser assertions aligned when changing domains.
  The current site assumes a domain root, not a Pages project subpath.
  Pages must use **GitHub Actions**, not branch/Jekyll publishing. Forks need
  their own domain and publication setup. CNAME does not configure DNS or
  enforce HTTPS.
- Keep the landing page and guides indexable: no `noindex` metadata or robots
  exclusion on those pages; the 404 page remains nonindexable. Sitemap entries
  must match the pages' canonical URLs, including `.html` for guides, and
  `robots.txt` should point to the sitemap. Check the built output for all three.

Use the [native docs checker](../../../docs/README.md#checks), including its
committed-snapshot mode for both docs-only and mixed patches, and the preview
procedure in that README. Keep the Rust catalog fixture and workflow checks
explicit as remaining CI coverage; the Linux aggregate is not required for
this native documentation path. Inspect navigation on desktop/mobile,
search, light/dark and system preference behavior, Mermaid SVG output, and links to source files.
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
