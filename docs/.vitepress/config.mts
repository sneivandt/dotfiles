import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { defineConfig, type DefaultTheme } from 'vitepress';
import { slug } from 'github-slugger';
import { themeStartup } from './theme-startup.mts';

const domain = readFileSync(new URL('../public/CNAME', import.meta.url), 'utf8').trim();
const origin = `https://${domain}`;
const siteTitle = 'Dotfiles';
const pageSources = Object.fromEntries(
  ['guides', 'reference', 'development'].flatMap((section) =>
    readdirSync(new URL(`../content/${section}/`, import.meta.url))
      .filter((name) => name.endsWith('.md'))
      .map((name) => [name.slice(0, -3), `${section}/${name}`]),
  ),
);
const page = (name: string): DefaultTheme.SidebarItem => ({
  text: readFileSync(new URL(`../content/${pageSources[name]}`, import.meta.url), 'utf8').match(/^title: (.+)$/m)![1],
  link: name === 'index' ? '/' : `/${name}`,
});
const group = (text: string, pages: string[]): DefaultTheme.SidebarItem => ({
  text,
  collapsed: false,
  items: pages.map(page),
});

const guide = [
  group('Using dotfiles', ['getting-started', 'commands-and-usage', 'profiles-and-categories', 'configuration-basics', 'security-and-trust', 'package-management', 'troubleshooting']),
  group('Platforms', ['arch-desktop', 'windows-and-wsl']),
  group('Customization', ['application-settings', 'agent-configuration', 'private-overlays']),
];
const reference = [
  page('cli-reference'),
  page('task-reference'),
  {
    text: 'Task details',
    collapsed: false,
    items: [
      'agent-packages', 'agent-settings', 'apm-plugin-checks', 'aur-packages',
      'configuration-diagnostics', 'custom-package-repository', 'custom-package-versions',
      'dotfiles-launcher', 'dotfiles-repository', 'file-permissions', 'git-hooks',
      'global-git-settings', 'home-symlinks', 'login-shell', 'overlay-scripts',
      'paru-package-manager', 'psscriptanalyzer', 'required-configuration-files',
      'shell-completions', 'shell-path', 'shellcheck', 'symlink-sources',
      'system-configuration-files', 'system-packages', 'systemd-units',
      'user-registry-settings', 'vs-code-extensions', 'windows-developer-mode',
    ].map(page),
  },
];
const contribute = [
  group('Workflows', ['contributing', 'writing-documentation']),
  group('Implementation', ['architecture', 'adding-a-task', 'debugging']),
  group('Verification', ['testing', 'writing-tests', 'mutation-testing', 'ci-and-releases']),
];
const sectionPages = (items: DefaultTheme.SidebarItem[]): string[] =>
  items.flatMap((item) => [
    ...(item.link ? [item.link.slice(1)] : []),
    ...sectionPages(item.items ?? []),
  ]);
const sectionMatch = (items: DefaultTheme.SidebarItem[]) =>
  `^/(${sectionPages(items).filter(Boolean).join('|')})(\\.html)?$`;
const sectionSidebars = (items: DefaultTheme.SidebarItem[]) =>
  Object.fromEntries(sectionPages(items).map((name) => [`/${name}`, items]));

export default defineConfig({
  title: siteTitle,
  description: 'Set up, customize, and maintain an Arch Linux or Windows machine with declarative configuration and a Rust CLI.',
  lang: 'en-US',
  srcDir: 'content',
  cleanUrls: true,
  rewrites: Object.fromEntries(Object.entries(pageSources).map(([name, source]) => [source, `${name}.md`])),
  outDir: 'dist',
  vite: {
    plugins: [themeStartup()],
    publicDir: resolve(import.meta.dirname, '../public'),
    // esbuild 0.28 cannot lower destructuring for Safari 14.0's implementation.
    build: { target: ['es2020', 'chrome87', 'edge88', 'firefox78', 'safari14.1'] },
  },
  appearance: 'dark',
  lastUpdated: true,
  head: [['link', { rel: 'icon', href: '/favicon.svg' }]],
  sitemap: {
    hostname: origin,
    transformItems: (items) => items.filter((item) => item.url !== '404' && item.url !== '404.html'),
  },
  async buildEnd(site) {
    writeFileSync(resolve(site.outDir, 'sitemap-index.xml'),
      `<?xml version="1.0" encoding="UTF-8"?><sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><sitemap><loc>${origin}/sitemap.xml</loc></sitemap></sitemapindex>`);
  },
  transformPageData(page) {
    if (page.relativePath === 'index.md') page.title = 'Dotfiles | Arch Linux & Windows setup with a Rust CLI';
  },
  transformHead({ pageData }) {
    const pagePath = pageData.relativePath.replace(/\.md$/, '');
    if (pagePath === '404') {
      return [
        ['meta', { name: 'robots', content: 'noindex, nofollow' }],
        ['meta', { property: 'og:site_name', content: siteTitle }],
      ];
    }
    const canonical = origin + (pagePath === 'index' ? '/' : `/${pagePath}`);
    return [
      ['link', { rel: 'canonical', href: canonical }],
      ['meta', { property: 'og:site_name', content: siteTitle }],
      ['meta', { property: 'og:url', content: canonical }],
      ['meta', { property: 'og:title', content: pageData.title }],
      ['meta', { property: 'og:description', content: pageData.description }],
    ];
  },
  markdown: {
    headers: { level: [2, 3] },
    anchor: { slugify: slug },
    config(md) {
      const linkOpen = md.renderer.rules.link_open!;
      md.renderer.rules.link_open = (tokens, index, options, env, self) => {
        const token = tokens[index];
        const href = token.attrGet('href');
        // Source links cross section folders; published routes remain flat.
        if (href) {
          token.attrSet('href', href.replace(
            /^(?:\.\.\/|\/)?(?:guides|reference|development)\/([^/?#]+\.md)([?#].*)?$/,
            '/$1$2',
          ));
        }
        return linkOpen(tokens, index, options, env, self);
      };
      const fence = md.renderer.rules.fence!;
      md.renderer.rules.fence = (tokens, index, options, env, self) => {
        const token = tokens[index];
        if (token.info.trim() === 'mermaid') {
          return `<MermaidDiagram source="${md.utils.escapeHtml(token.content)}" />`;
        }
        return fence(tokens, index, options, env, self);
      };
    },
  },
  themeConfig: {
    logo: { light: '/assets/logo-light.svg', dark: '/assets/logo-dark.svg', alt: '' },
    nav: [
      { text: 'Guides', link: '/getting-started', activeMatch: sectionMatch(guide) },
      { text: 'Reference', link: '/cli-reference', activeMatch: sectionMatch(reference) },
      { text: 'Development', link: '/contributing', activeMatch: sectionMatch(contribute) },
    ],
    sidebar: {
      ...sectionSidebars(reference),
      ...sectionSidebars(contribute),
      '/': guide,
    },
    outline: { level: [2, 3], label: 'On this page' },
    search: {
      provider: 'local',
      options: {
        _render(source, env, md) {
          const html = md.render(source, env);
          if (env.frontmatter?.search === false) return '';
          const description = env.frontmatter?.description;
          if (typeof description !== 'string' || !description.trim()) {
            throw new Error(`${env.relativePath}: search requires a nonempty page description`);
          }
          // Index the page summary in its introduction, not in every section.
          const summary = `<p>${md.utils.escapeHtml(description)}</p>`;
          if (/<h1\b[^>]*>[\s\S]*?<\/h1>/i.test(html)) {
            return html.replace(/<h1\b[^>]*>[\s\S]*?<\/h1>/i, (heading) => `${heading}\n${summary}`);
          }
          const title = typeof env.frontmatter?.title === 'string' && env.frontmatter.title.trim()
            ? `<h1>${md.utils.escapeHtml(env.frontmatter.title)}</h1>\n`
            : '';
          return `${title}${summary}\n${html}`;
        },
      },
    },
    socialLinks: [{ icon: 'github', link: 'https://github.com/sneivandt/dotfiles' }],
    editLink: {
      pattern: ({ filePath }) => `https://github.com/sneivandt/dotfiles/edit/main/docs/content/${filePath}`,
    },
    docFooter: { prev: 'Previous page', next: 'Next page' },
    notFound: { title: 'Page not found', quote: 'The page may have moved, or the address may be incomplete.', linkText: 'Back to the docs' },
  },
});
