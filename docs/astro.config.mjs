import { readFileSync } from 'node:fs';
import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import sitemap from '@astrojs/sitemap';
import starlight from '@astrojs/starlight';
import mermaid from 'astro-mermaid';
import { remarkDocs } from './src/remark-docs.mjs';

const domain = readFileSync(new URL('./public/CNAME', import.meta.url), 'utf8').trim();

export default defineConfig({
  site: `https://${domain}`,
  trailingSlash: 'never',
  build: { format: 'file' },
  markdown: {
    processor: unified({ remarkPlugins: [remarkDocs] }),
  },
  integrations: [
    sitemap({
      serialize(item) {
        const url = new URL(item.url);
        if (url.pathname !== '/' && !url.pathname.endsWith('.html')) {
          url.pathname += url.pathname.endsWith('/') ? 'index.html' : '.html';
        }
        return { ...item, url: url.href };
      },
    }),
    mermaid({
      autoTheme: true,
      enableLog: false,
      mermaidConfig: {
        securityLevel: 'strict',
        fontFamily: 'ui-sans-serif, system-ui, sans-serif',
        flowchart: {
          useMaxWidth: false,
          curve: 'linear',
          nodeSpacing: 24,
          rankSpacing: 24,
          padding: 8,
          wrappingWidth: 240,
        },
      },
    }),
    starlight({
      title: 'Dotfiles',
      description: 'Set up, customize, and maintain an Arch Linux or Windows machine with declarative configuration and a Rust CLI.',
      favicon: '/favicon.svg',
      logo: { src: './public/favicon.svg' },
      credits: false,
      disable404Route: true,
      components: { ThemeProvider: './src/components/ThemeProvider.astro' },
      head: [{ tag: 'meta', attrs: { name: 'robots', content: 'index, follow' } }],
      social: [
        { icon: 'github', label: 'GitHub repository', href: 'https://github.com/sneivandt/dotfiles' },
      ],
      editLink: { baseUrl: 'https://github.com/sneivandt/dotfiles/edit/main/docs/' },
      customCss: [
        'starlight-theme-flexoki/styles/layers.css',
        'starlight-theme-flexoki/styles/theme.css',
        'starlight-theme-flexoki/styles/base.css',
        'starlight-theme-flexoki/accent-themes/cyan.css',
        './src/styles/custom.css',
      ],
      sidebar: [
        {
          label: 'Start here',
          items: ['getting-started', 'profiles-and-categories'],
        },
        {
          label: 'Use dotfiles',
          items: ['commands-and-usage', 'update-lifecycle', 'troubleshooting', 'task-reference', 'security-and-trust'],
        },
        {
          label: 'Customize',
          items: ['configuration-reference', 'private-overlays', 'application-settings'],
        },
        {
          label: 'Subsystem guides',
          items: [
            {
              label: 'AI agents',
              items: ['ai-agent-configuration', 'agent-deployment-internals'],
            },
            {
              label: 'Packages',
              items: ['package-management', 'local-package-recipes'],
            },
            'arch-desktop', 'windows-and-wsl', 'visual-studio-code', 'vim-and-neovim',
          ],
        },
        {
          label: 'Development',
          items: ['contributing', 'architecture', 'testing', 'git-hooks', 'ci-and-releases'],
        },
      ],
    }),
  ],
});
