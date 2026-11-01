---
name: docs-publishing
description: >
  Use for the published documentation site's Jekyll configuration under docs/,
  theme, navigation, search, Mermaid rendering, custom domain, or Pages publishing.
  Not for ordinary guide prose or the CLI binary release workflow.
---

# Documentation Publishing

Start with [site configuration](../../../docs/_config.yml),
[domain declaration](../../../docs/CNAME), and the
[publishing guide](../../../docs/CONTRIBUTING.md#published-documentation).
GitHub Pages builds `docs/` from `main` with Jekyll; preserve that deployment
path unless a migration is requested. The documentation site is independent of
CLI CI and binary releases. The root README directs readers to the published
guides; keep its URLs and heading anchors aligned with the site's output.

- Keep the theme pinned. Use its supported configuration for the dark theme,
  navigation, search, code buttons, and Mermaid before adding custom JavaScript.
  Fenced `mermaid` blocks need the theme's renderer; GitHub's repository rendering
  does not prove the website renders them.
- Preserve Markdown filenames and incoming URLs. Each guide supplies `title`
  and unique `nav_order` front matter. The home page is `docs/README.md` with
  `permalink: /`; keep README and CONTRIBUTING explicitly included by Jekyll.
- Keep guide links relative with `.md` extensions for Jekyll's relative-links
  plugin. Repository files outside `docs/` need GitHub links on the site.
- Keep `url`, `baseurl`, CNAME, and Pages settings consistent. Forks need their
  own domain configuration. A CNAME file does not create DNS records or enforce
  HTTPS; those are separate provider/Pages settings.

Use the [documentation checks](../../../docs/TESTING.md#choosing-coverage) and
preview procedure in the publishing guide. Inspect navigation on desktop/mobile,
search, Mermaid SVG output, and links to source files. Report unavailable local
rendering checks rather than treating a Markdown link check as a rendered-site
check.

After an authorized push, follow Pages for the exact commit as well as the
repository CI aggregate and informational results. Verify the deployed site,
HTTPS, and HTTP-to-HTTPS redirect separately. A successful build alone does not
prove deployment or browser-side rendering. DNS changes, deployment, and agent
package installation remain subject to the user's requested scope.
