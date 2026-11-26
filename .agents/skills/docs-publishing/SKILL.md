---
name: docs-publishing
description: >
  Use for documentation organization and site management: adding or moving
  docs/content/ pages, README/site links, VitePress navigation, themes,
  search, diagrams/assets, domain configuration, site checks, or Pages
  publishing. Not for prose-only editing (docs-writing), internal agent
  instructions (skill-maintenance), or CLI binary releases (ci-cd-patterns).
---

# Documentation Management and Publishing

[docs/README.md](../../../docs/README.md) owns preview, checks, and site
architecture; [config.mts](../../../docs/.vitepress/config.mts) owns the
navigation inventory; [Writing documentation](../../../docs/content/development/writing-documentation.md)
and [docs-writing](../docs-writing/SKILL.md) own topic ownership and prose.

## Coupled site contracts

- **Pages & routes**: Keep guide URLs flat with matching title, filename, and
  route; update incoming links on moves and never add rename-compatibility
  routes. Keep the 404 page excluded from search indexing.
- **Theme, layout, & assets**: Prefer native VitePress controls and theme
  persistence. Verify *content* fits at phone width (shorten labels or
  reorganize rather than shrinking text). Keep `docs/public/assets/` (including
  `terminal-screenshot.svg`) aligned with `docs/tests/browser/site.spec.ts`.
- **Domain & publication**: Keep `CNAME`, canonical/OG/structured metadata,
  `robots.txt`, sitemap, `README.md` URLs, and browser assertions aligned.
  Pages must deploy the same-run verified CI artifact from a successful push on
  `main`, reject superseded runs, and never execute a checkout with deployment
  credentials. Verify HTTPS and HTTP redirects separately for domain changes.

## Checks

Select affected checks from [docs/README.md](../../../docs/README.md#checks).
Never share `docs/node_modules` across Linux and Windows. For publication
monitoring, follow [ci-cd-patterns](../ci-cd-patterns/SKILL.md#after-pushing).
Site edits do not authorize DNS, account, or Pages settings changes.
