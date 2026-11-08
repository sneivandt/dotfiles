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

Start with [docs/README.md](../../../docs/README.md), which owns preview,
checks, site implementation, navigation, and publication procedures.
[Site configuration](../../../docs/.vitepress/config.mts) owns the current
navigation inventory and order; do not reproduce that taxonomy in this skill.
Use [Writing documentation](../../../docs/content/development/writing-documentation.md#choose-the-owning-page)
for topic ownership and [docs-writing](../docs-writing/SKILL.md) for prose.

## Choose the affected surface

| Change | Inspect and preserve |
|---|---|
| New or moved page | Topic ownership, title/filename/route alignment, useful descriptions, incoming links, representative search queries, outline, and navigation coverage |
| Search metadata | Native local-search indexing, escaped introduction summaries, section destinations, canonical metadata, and 404 exclusion |
| Navigation | Direct-link and search destinations, active section, mobile controls, and independent overview visibility |
| Theme or diagrams | Native VitePress behavior, theme persistence, readable text, contained scrolling, keyboard focus, contrast, and diagram accessibility |
| Domain or publication | CNAME, canonical/social/structured metadata, robots, sitemap, README URLs, browser expectations, and the same-run artifact contract |
| Dependencies or checks | Pinned dependencies, native toolchain, focused regression fixtures, and CI selection/coverage |

Keep the landing page's first action prominent and its setup walkthrough in
Getting started. Keep the root README brief. Split pages by reader task or
audience, not length. Keep guide URLs flat and update references after moves;
do not add rename-compatibility routes.
Use the human guide's [findability conventions](../../../docs/content/development/writing-documentation.md#help-readers-find-the-topic);
do not invent a separate keyword/tag taxonomy or copy the page inventory here.

Prefer VitePress's native theme and controls. Preserve a reader's appearance
choice, diagram titles/descriptions, warning information, and readable tables.
Do not shrink text to hide overflow. Tests should protect these contracts,
not incidental sentences or hardcoded inventory counts.

## Verify and publish within scope

Select affected checks from [docs/README.md](../../../docs/README.md#checks).
Build for changed routes, rendering, generated metadata, assets, or dependencies;
use focused browser tests for affected search, navigation, layout, or interaction.
Use the full native checker for broad site changes or uncertain impact.
Never share node_modules across Windows and Linux.

When rendered behavior changes, review the affected path on desktop and narrow
screens, including keyboard navigation, search dismissal, focus, links, warnings, and diagrams
where affected. Browser emulation is not native phone or screen-reader proof.
Repository source URLs can be checked locally; upstream destinations require
separate review and must not receive private content.

For publication monitoring, use
[ci-cd-patterns](../ci-cd-patterns/SKILL.md#after-pushing). Pages must deploy
the verified artifact from a successful same-repository push on main, reject
superseded runs, and avoid executing a checkout with deployment credentials.
For domain changes, verify HTTPS and the HTTP redirect separately.
Site edits do not authorize DNS, account settings, deployment, or host changes.
