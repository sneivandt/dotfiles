import { preview } from 'astro';

await preview({ server: { host: 'localhost', port: Number(process.env.DOCS_TEST_PORT ?? 4322) } });
