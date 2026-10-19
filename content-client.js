import seed from './content.seed.json';
import { escapeHtml } from './menu-renderer.js';

const local = ['localhost', '127.0.0.1', '[::1]'].includes(location.hostname);
export const CONTENT_ORIGIN = (import.meta.env.VITE_CONTENT_API_URL ?? (local ? '' : 'https://dashboard.lacocinamexicana.net')).replace(/\/$/, '');
export function imageUrl(path) { return path.startsWith('/uploads/') ? `${CONTENT_ORIGIN}${path}` : path; }
export async function getPublishedContent() {
  const response = await fetch(`${CONTENT_ORIGIN}/api/content`, { cache: 'no-store', credentials: 'omit', signal: AbortSignal.timeout(5000) });
  if (!response.ok) throw new Error('Content unavailable');
  const content = await response.json();
  if (!Array.isArray(content.menu_items) || !Array.isArray(content.gallery) || !Number.isInteger(content.revision)) throw new Error('Invalid content');
  return content;
}
export async function initialContent() {
  try { return await getPublishedContent(); } catch { return structuredClone(seed); }
}
export function renderGallery(gallery) {
  return gallery.map((photo, i) => `<div class="gallery-item${i > 2 ? ' desktop-hidden' : ''}"><img src="${escapeHtml(imageUrl(photo.url))}" alt="${escapeHtml(photo.alt)}" loading="lazy"></div>`).join('');
}
