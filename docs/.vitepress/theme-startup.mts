import type { Plugin } from 'vite';

// VitePress 1.6.4 defers these values until client startup. Keep the native
// components, but render their initial layout and outline during SSR as well.
// Fail on upstream changes so an upgrade cannot silently restore the flicker.
export function themeStartup(): Plugin {
  return {
    name: 'dotfiles-theme-startup',
    enforce: 'pre',
    transform(source, id) {
      const path = id.replaceAll('\\', '/');
      if (path.includes('?') || !path.includes('/vitepress/dist/client/theme-default/components/')) return;
      const replace = (before: string, after: string) => {
        if (!source.includes(before)) throw new Error(`Review VitePress startup workaround: ${path}`);
        source = source.replace(before, after);
      };
      if (path.endsWith('/VPNavBar.vue')) {
        replace("import { ref, watchPostEffect } from 'vue'", "import { computed } from 'vue'");
        replace('const classes = ref<Record<string, boolean>>({})\n\nwatchPostEffect(() => {\n  classes.value = {',
          'const classes = computed(() => {\n  return {');
      } else if (path.endsWith('/VPDocAsideOutline.vue')) {
        replace('const { frontmatter, theme } = useData()', 'const { frontmatter, theme, page } = useData()');
        replace('const headers = shallowRef<MenuItem[]>([])',
          'const headers = shallowRef<MenuItem[]>(frontmatter.value.outline === false ? [] : page.value.headers)');
      } else return;
      return source;
    },
  };
}
