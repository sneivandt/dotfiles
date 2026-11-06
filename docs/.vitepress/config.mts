import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { defineConfig, type DefaultTheme } from 'vitepress';
import { slug } from 'github-slugger';

const domain = readFileSync(new URL('../public/CNAME', import.meta.url), 'utf8').trim();
const origin = `https://${domain}`;
const page = (name: string): DefaultTheme.SidebarItem => ({
  text: readFileSync(new URL(`../content/${name}.md`, import.meta.url), 'utf8').match(/^title: (.+)$/m)![1],
  link: `/${name}.html`,
});
const group = (text: string, pages: string[]): DefaultTheme.SidebarItem => ({
  text,
  collapsed: false,
  items: pages.map(page),
});

const guide = [
  group('Introduction', ['getting-started']),
  group('Using dotfiles', ['commands-and-usage', 'profiles-and-categories', 'configuration-basics', 'security-and-trust', 'package-management', 'troubleshooting']),
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
  group('Implementation', ['architecture', 'adding-a-task', 'debugging', 'arch-package-recipes']),
  group('Verification', ['testing', 'writing-tests', 'mutation-testing', 'ci-and-releases']),
];
const sectionPages = (items: DefaultTheme.SidebarItem[]): string[] =>
  items.flatMap((item) => [
    ...(item.link ? [item.link.slice(1, -5)] : []),
    ...sectionPages(item.items ?? []),
  ]);
const sectionMatch = (items: DefaultTheme.SidebarItem[]) =>
  `^/(${sectionPages(items).join('|')})(\\.html)?$`;
const sectionSidebars = (items: DefaultTheme.SidebarItem[]) =>
  Object.fromEntries(sectionPages(items).map((name) => [`/${name}`, items]));

export default defineConfig({
  title: 'Dotfiles',
  description: 'Set up, customize, and maintain an Arch Linux or Windows machine with declarative configuration and a Rust CLI.',
  lang: 'en-US',
  srcDir: 'content',
  outDir: 'dist',
  vite: {
    publicDir: resolve(import.meta.dirname, '../public'),
    // esbuild 0.28 cannot lower destructuring for Safari 14.0's implementation.
    build: { target: ['es2020', 'chrome87', 'edge88', 'firefox78', 'safari14.1'] },
  },
  appearance: 'dark',
  lastUpdated: true,
  head: [['link', { rel: 'icon', href: '/favicon.svg' }]],
  sitemap: {
    hostname: origin,
    transformItems: (items) => items.filter((item) => item.url !== '404.html'),
  },
  async buildEnd(site) {
    writeFileSync(resolve(site.outDir, 'sitemap-index.xml'),
      `<?xml version="1.0" encoding="UTF-8"?><sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><sitemap><loc>${origin}/sitemap.xml</loc></sitemap></sitemapindex>`);
  },
  transformPageData(page) {
    if (page.relativePath === 'index.md') page.title = 'Dotfiles by Stuart Neivandt';
  },
  transformHead({ pageData }) {
    const path = pageData.relativePath.replace(/\.md$/, '.html');
    const canonical = origin + (path === 'index.html' ? '/' : `/${path}`);
    if (path === '404.html') return [['meta', { name: 'robots', content: 'noindex, nofollow' }]];
    return [
      ['link', { rel: 'canonical', href: canonical }],
      ['meta', { property: 'og:url', content: canonical }],
      ['meta', { property: 'og:title', content: pageData.title }],
      ['meta', { property: 'og:description', content: pageData.description }],
    ];
  },
  markdown: {
    anchor: { slugify: slug },
    config(md) {
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
    logo: '/favicon.svg',
    nav: [
      { text: 'Guides', link: '/getting-started.html', activeMatch: sectionMatch(guide) },
      { text: 'Reference', link: '/cli-reference.html', activeMatch: sectionMatch(reference) },
      { text: 'Development', link: '/contributing.html', activeMatch: sectionMatch(contribute) },
    ],
    sidebar: {
      ...sectionSidebars(reference),
      ...sectionSidebars(contribute),
      '/': guide,
    },
    outline: { level: [2, 3], label: 'On this page' },
    search: { provider: 'local' },
    socialLinks: [{ icon: 'github', link: 'https://github.com/sneivandt/dotfiles' }],
    editLink: { pattern: 'https://github.com/sneivandt/dotfiles/edit/main/docs/content/:path' },
    docFooter: { prev: 'Previous page', next: 'Next page' },
    notFound: { title: 'Page not found', quote: 'The page may have moved, or the address may be incomplete.', linkText: 'Back to the docs' },
  },
});
