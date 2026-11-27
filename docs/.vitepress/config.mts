import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { defineConfig, type DefaultTheme, type HeadConfig } from 'vitepress';
import { slug } from 'github-slugger';
import { themeStartup } from './theme-startup.mts';

const domain = readFileSync(new URL('../public/CNAME', import.meta.url), 'utf8').trim();
const origin = `https://${domain}`;
const siteTitle = 'Dotfiles';
const authorName = 'Stuart Neivandt';
const authorUrl = 'https://stuartneivandt.com/';
const authorId = `${authorUrl}#person`;
const repositoryUrl = 'https://github.com/sneivandt/dotfiles';
const ogImage = `${origin}/assets/og-image.png`;
const ogImageAlt = 'Dotfiles: Arch Linux and Windows setup managed with a Rust CLI by Stuart Neivandt';
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
  group('Introduction', ['getting-started', 'commands-and-usage', 'profiles-and-categories', 'configuration-basics']),
  group('Customization', ['package-management', 'application-settings', 'agent-configuration', 'private-overlays']),
  group('Platforms', ['arch-desktop', 'windows-and-wsl']),
  group('Safety and troubleshooting', ['security-and-trust', 'troubleshooting']),
];
const reference = [
  page('cli-reference'),
  page('task-reference'),
  group('Update tasks', [
    'agent-packages', 'agent-settings', 'aur-packages',
    'custom-package-repository', 'custom-package-versions',
    'dotfiles-launcher', 'dotfiles-repository', 'file-permissions', 'git-hooks',
    'global-git-settings', 'home-symlinks', 'login-shell', 'overlay-scripts',
    'paru-package-manager', 'shell-completions', 'shell-path',
    'system-configuration-files', 'system-packages', 'systemd-units',
    'user-registry-settings', 'vs-code-extensions', 'windows-developer-mode',
  ]),
  group('Validation tasks', [
    'apm-plugin-checks', 'configuration-diagnostics',
    'required-configuration-files', 'symlink-sources',
  ]),
];
const contribute = [
  group('Overview', ['contributing', 'architecture']),
  group('Implementation', ['adding-a-task', 'debugging', 'writing-documentation']),
  group('Testing and releases', ['testing', 'writing-tests', 'mutation-testing', 'ci-and-releases']),
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
  head: [
    ['meta', { name: 'author', content: authorName }],
    ['meta', { name: 'theme-color', content: '#1a1b26' }],
    ['link', { rel: 'icon', href: '/favicon.svg', type: 'image/svg+xml' }],
    ['link', { rel: 'icon', href: '/favicon.png', type: 'image/png', sizes: '96x96' }],
    ['link', { rel: 'alternate icon', href: '/favicon.ico' }],
    ['link', { rel: 'apple-touch-icon', href: '/apple-touch-icon.png', sizes: '180x180' }],
  ],
  sitemap: {
    hostname: origin,
    transformItems: (items) => items.filter((item) => item.url === ''),
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
    const isHome = pagePath === 'index';
    const canonical = origin + (isHome ? '/' : `/${pagePath}`);
    const head: HeadConfig[] = [
      ['meta', { name: 'robots', content: isHome ? 'index, follow, max-image-preview:large' : 'noindex, follow' }],
      ['link', { rel: 'canonical', href: canonical }],
      ['meta', { property: 'og:type', content: isHome ? 'website' : 'article' }],
      ['meta', { property: 'og:site_name', content: siteTitle }],
      ['meta', { property: 'og:locale', content: 'en_US' }],
      ['meta', { property: 'og:url', content: canonical }],
      ['meta', { property: 'og:title', content: pageData.title }],
      ['meta', { property: 'og:description', content: pageData.description }],
      ['meta', { property: 'og:image', content: ogImage }],
      ['meta', { property: 'og:image:secure_url', content: ogImage }],
      ['meta', { property: 'og:image:type', content: 'image/png' }],
      ['meta', { property: 'og:image:width', content: '1200' }],
      ['meta', { property: 'og:image:height', content: '630' }],
      ['meta', { property: 'og:image:alt', content: ogImageAlt }],
      ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
      ['meta', { name: 'twitter:title', content: pageData.title }],
      ['meta', { name: 'twitter:description', content: pageData.description }],
      ['meta', { name: 'twitter:image', content: ogImage }],
      ['meta', { name: 'twitter:image:alt', content: ogImageAlt }],
    ];
    if (isHome) {
      head.push([
        'script',
        { type: 'application/ld+json' },
        JSON.stringify({
          '@context': 'https://schema.org',
          '@graph': [
            {
              '@type': 'WebSite',
              '@id': `${origin}/#website`,
              url: `${origin}/`,
              name: siteTitle,
              description: pageData.description,
              inLanguage: 'en-US',
              author: { '@id': authorId },
              creator: { '@id': authorId },
            },
            {
              '@type': 'SoftwareSourceCode',
              '@id': `${origin}/#software`,
              name: siteTitle,
              description: pageData.description,
              url: `${origin}/`,
              codeRepository: repositoryUrl,
              programmingLanguage: 'Rust',
              runtimePlatform: ['Arch Linux', 'Windows'],
              author: { '@id': authorId },
              creator: { '@id': authorId },
            },
            {
              '@type': 'Person',
              '@id': authorId,
              name: authorName,
              alternateName: 'sneivandt',
              url: authorUrl,
              sameAs: [
                'https://github.com/sneivandt',
                'https://www.linkedin.com/in/stuartneivandt',
              ],
            },
          ],
        }),
      ]);
    }
    return head;
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
          token.attrSet('href', href
            .replace(/^(?:\.\.\/|\/)?(?:guides|reference|development)\/([^/?#]+\.md)([?#].*)?$/, '/$1$2')
            .replace(/^\.\.\/(index\.md)([?#].*)?$/, '/$1$2'));
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
