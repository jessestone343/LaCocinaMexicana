export const CATEGORIES = ['Starters', 'Lunch Specials', 'Entrées', 'Especialidades', "Children's Menu", 'Desserts'];
export const escapeHtml = (value) => String(value).replace(/[&<>"']/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char]));

// Both the restaurant and dashboard use this renderer and the same menu CSS.
export function renderMenu(items, today = new Date().toLocaleDateString('en-US', { weekday: 'long' })) {
  return CATEGORIES.map((category) => {
    const dishes = items.filter((item) => item.category === category);
    if (!dishes.length) return '';
    const id = 'menu-' + category.toLowerCase().normalize('NFD').replace(/[\u0300-\u036f']/g, '').replace(/\s+/g, '-');
    return `<div id="${id}" class="menu-category fade-in visible">
      <h3 class="menu-category-title">${escapeHtml(category)}</h3>
      ${category === "Children's Menu" ? '<p class="menu-category-note">All meals are served with rice and beans</p>' : ''}
      <div class="menu-grid">${dishes.map((item) => {
        const special = item.name === `Lunch Special - ${today}`;
        return `<div class="menu-item${special ? ' highlight-special' : ''}" data-menu-id="${escapeHtml(item.id)}">
          <div class="menu-item-header"><span class="menu-item-name">${escapeHtml(item.name.replace('Lunch Special - ', ''))}${item.vegetarian ? ' <span class="veg-indicator" title="Vegetarian">(V)</span>' : ''}${special ? ' <span class="special-badge">Today\'s Special</span>' : ''}</span>
          <span class="menu-item-dots"></span><span class="menu-item-price">${escapeHtml(item.price)}</span></div>
          ${item.description ? `<p class="menu-item-desc">${escapeHtml(item.description)}</p>` : ''}</div>`;
      }).join('')}</div></div>`;
  }).join('') || '<p class="empty-menu">Our menu is being updated. Please check back soon.</p>';
}
