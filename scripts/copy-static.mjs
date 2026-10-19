import { cp } from 'node:fs/promises';
await cp('images', 'dist/images', { recursive: true });
