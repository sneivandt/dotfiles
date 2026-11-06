import DefaultTheme from 'vitepress/theme';
import type { Theme } from 'vitepress';
import { defineComponent, h, onMounted, onUnmounted } from 'vue';
import MermaidDiagram from './MermaidDiagram.vue';
import './style.css';

export default {
  extends: DefaultTheme,
  Layout: defineComponent({
    setup() {
      let lastOutsideFocus: HTMLElement | null = null;
      let pendingRestore: number | undefined;
      function rememberFocus(event: FocusEvent) {
        if (event.target instanceof HTMLElement && !event.target.closest('.VPLocalSearchBox')) {
          lastOutsideFocus = event.target;
        }
      }
      function restoreSearchFocus(event: KeyboardEvent) {
        if (event.key !== 'Escape' || !(event.target instanceof HTMLElement) ||
            !event.target.closest('.VPLocalSearchBox')) return;
        const target = lastOutsideFocus ?? document.querySelector<HTMLElement>('#local-search button');
        // VitePress 1.6 activates its trap after focusing the input, so its
        // saved return target belongs to the removed modal rather than its opener.
        // Restore after native Escape handlers finish dispatching and remove it.
        pendingRestore = requestAnimationFrame(() => {
          pendingRestore = undefined;
          if (!document.querySelector('.VPLocalSearchBox') && target?.isConnected) target.focus();
        });
      }
      onMounted(() => {
        document.addEventListener('focusin', rememberFocus);
        document.addEventListener('keydown', restoreSearchFocus, true);
      });
      onUnmounted(() => {
        if (pendingRestore !== undefined) cancelAnimationFrame(pendingRestore);
        document.removeEventListener('focusin', rememberFocus);
        document.removeEventListener('keydown', restoreSearchFocus, true);
      });
      return () => h(DefaultTheme.Layout);
    },
  }),
  enhanceApp({ app }) {
    app.component('MermaidDiagram', MermaidDiagram);
  },
} satisfies Theme;
