import { slug } from 'github-slugger';
import { visit } from 'unist-util-visit';

export function publishedLink(url) {
  if (/^(?:[a-z][a-z\d+.-]*:|\/\/|#)/i.test(url)) return url;
  const match = /^([^?#]+)\.md([?#].*)?$/.exec(url);
  if (!match) return url;
  const [, path, suffix = ''] = match;
  return `${path}.html${suffix}`;
}

function headingText(node) {
  return node.value ?? node.children?.map(headingText).join('') ?? '';
}

export function remarkDocs() {
  return (tree) => {
    // Starlight supplies the visible H1; keep canonical Markdown title fragments working.
    const index = tree.children.findIndex((node) => node.type === 'heading' && node.depth === 1);
    if (index !== -1) {
      const heading = tree.children[index];
      tree.children[index] = {
        type: 'html',
        value: `<span id="${slug(headingText(heading))}" class="page-title-anchor"></span>`,
      };
    }
    visit(tree, ['link', 'definition'], (node) => {
      node.url = publishedLink(node.url);
    });
  };
}
