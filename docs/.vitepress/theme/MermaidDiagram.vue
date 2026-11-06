<script lang="ts">
let renderQueue = Promise.resolve();
</script>

<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from 'vue';
import { useData } from 'vitepress';

const props = defineProps<{ source: string }>();
const { isDark } = useData();
const container = ref<HTMLElement>();
const error = ref('');
// Mermaid has shared configuration. Serialize rendering across diagrams and theme changes.
let revision = 0;
let mounted = false;
function render() {
  const current = ++revision;
  renderQueue = renderQueue.then(async () => {
    if (!mounted || current !== revision || !container.value) return;
    try {
      const { default: mermaid } = await import('mermaid');
      mermaid.initialize({
        startOnLoad: false, securityLevel: 'strict',
        theme: isDark.value ? 'dark' : 'default',
        fontFamily: 'ui-sans-serif, system-ui, sans-serif',
        flowchart: { useMaxWidth: false, curve: 'linear', nodeSpacing: 24, rankSpacing: 24, padding: 8, wrappingWidth: 240 },
      });
      const id = `diagram-${crypto.randomUUID()}`;
      const { svg } = await mermaid.render(id, props.source);
      if (current === revision && container.value) {
        container.value.innerHTML = svg;
        error.value = '';
      }
    } catch (cause) {
      error.value = 'Unable to render diagram.';
      console.error(cause);
    }
  });
}
onMounted(() => { mounted = true; render(); });
watch([isDark, () => props.source], render);
onBeforeUnmount(() => { mounted = false; revision++; });
</script>

<template>
  <div class="mermaid" ref="container"></div>
  <p v-if="error" role="alert">{{ error }}</p>
</template>
