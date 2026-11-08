import { serve } from 'vitepress';

await serve({ root: process.cwd(), port: Number(process.env.DOCS_TEST_PORT ?? 8000) });
