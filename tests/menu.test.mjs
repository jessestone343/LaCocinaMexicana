import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
// This repo keeps browser modules as .js without changing the existing Vercel
// function's CommonJS package. Import the standalone renderer as an ES module.
const source = await readFile(new URL('../menu-renderer.js', import.meta.url), 'utf8');
const { renderMenu, CATEGORIES } = await import(`data:text/javascript,${encodeURIComponent(source)}`);
const item = { id: 'dish-1', name: 'Taco', price: '$12', description: 'Fresh & good', vegetarian: true, category: 'Starters' };

test('menu remains correctly categorized after insertion and deletion', () => {
  const added = { ...item, id: 'dish-2', name: 'Flan', category: 'Desserts' };
  assert.match(renderMenu([added]), /id="menu-desserts"/);
  assert.doesNotMatch(renderMenu([added]), /id="menu-starters"/);
  assert.ok(renderMenu([added, item]).indexOf('Taco') < renderMenu([added, item]).indexOf('Flan'));
  assert.equal(CATEGORIES.length, 6);
});
test('all editable fields are escaped in the shared preview and public renderer', () => {
  const html = renderMenu([{ ...item, name: '<img src=x onerror=alert(1)>', description: '<script>alert(1)</script>', price: '<b>$5</b>' }]);
  assert.doesNotMatch(html, /<script>|<img|<b>/);
  assert.match(html, /&lt;script&gt;/);
  assert.match(html, /&lt;b&gt;\$5/);
});
test('specials, vegetarian labels, and flexible prices are preserved', () => {
  const html = renderMenu([{ ...item, name: 'Lunch Special - Monday', category: 'Lunch Specials', price: '$18 Single / $35 Double' }], 'Monday');
  assert.match(html, /highlight-special/);
  assert.match(html, /Vegetarian/);
  assert.match(html, /\$18 Single \/ \$35 Double/);
});
test('seed includes stable, unique IDs, explicit categories, and existing gallery images', async () => {
  const seed = JSON.parse(await readFile(new URL('../content.seed.json', import.meta.url)));
  assert.equal(new Set(seed.menu_items.map((i) => i.id)).size, seed.menu_items.length);
  for (const item of seed.menu_items) assert.ok(CATEGORIES.includes(item.category));
  for (const image of seed.gallery) await readFile(new URL(`..${image.url}`, import.meta.url));
});
