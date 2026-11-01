import { readFileSync } from 'node:fs';
import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import starlight from '@astrojs/starlight';
import mermaid from 'astro-mermaid';
import { remarkDocs } from './src/remark-docs.mjs';

const domain = readFileSync(new URL('./public/CNAME', import.meta.url), 'utf8').trim();

export default defineConfig({
  site: `https://${domain}`,
  trailingSlash: 'never',
  build: { format: 'file' },
  redirects: { '/': '/getting-started.html' },
  markdown: {
    processor: unified({ remarkPlugins: [remarkDocs] }),
  },
  integrations: [
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
      social: [
        { icon: 'github', label: 'GitHub repository', href: 'https://github.com/sneivandt/dotfiles' },
      ],
      editLink: { baseUrl: 'https://github.com/sneivandt/dotfiles/edit/main/docs/' },
      customCss: ['./src/styles/custom.css'],
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
          items: ['package-management', 'arch-desktop', 'windows-and-wsl', 'visual-studio-code', 'vim-and-neovim', 'ai-agent-configuration'],
        },
        {
          label: 'Development',
          items: ['contributing', 'architecture', 'testing', 'git-hooks', 'ci-and-releases'],
        },
      ],
    }),
  ],
});
