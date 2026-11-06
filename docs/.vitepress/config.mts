import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { defineConfig } from 'vitepress';
import { slug } from 'github-slugger';

const domain = readFileSync(new URL('../public/CNAME', import.meta.url), 'utf8').trim();
const origin = `https://${domain}`;
const group = (text: string, pages: string[]) => ({
  text,
  collapsed: false,
  items: pages.map((name) => ({
    text: readFileSync(new URL(`../content/${name}.md`, import.meta.url), 'utf8').match(/^title: (.+)$/m)![1],
    link: `/${name}.html`,
  })),
});

const guide = [
  group('Introduction', ['getting-started', 'profiles-and-categories', 'security-and-trust']),
  group('Workflows', ['commands-and-usage', 'package-management', 'troubleshooting']),
  group('Platforms', ['arch-desktop', 'windows-and-wsl']),
  group('Customization', ['agent-configuration', 'application-settings', 'private-overlays']),
];
const reference = [
  group('Reference', ['cli-reference', 'task-reference', 'configuration-basics']),
  group('Checkout and shell', ['dotfiles-repository', 'dotfiles-launcher', 'shell-path', 'git-hooks', 'shell-completions', 'global-git-settings', 'login-shell']),
  group('Files and Windows', ['home-symlinks', 'file-permissions', 'system-configuration-files', 'windows-developer-mode', 'user-registry-settings']),
  group('Packages and services', ['system-packages', 'paru-package-manager', 'aur-packages', 'custom-package-versions', 'custom-package-repository', 'systemd-units', 'vs-code-extensions']),
  group('Agents and overlays', ['agent-settings', 'agent-packages', 'overlay-scripts']),
  group('Validation', ['configuration-diagnostics', 'symlink-sources', 'configuration-files', 'apm-plugin-checks', 'shellcheck', 'psscriptanalyzer']),
];
const contribute = [group('Development', ['contributing', 'testing', 'architecture', 'ci-and-releases'])];
const sectionMatch = (groups: ReturnType<typeof group>[]) =>
  `^/(${groups.flatMap((section) => section.items.map((item) => item.link.slice(1, -5))).join('|')})(\\.html)?$`;
const sectionSidebars = (groups: ReturnType<typeof group>[], grouped = false) => {
  const pages = groups.flatMap((section) => section.items);
  return Object.fromEntries(pages.map((item) => [item.link.slice(0, -5), grouped ? groups : pages]));
};

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
      ...sectionSidebars(reference, true),
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
