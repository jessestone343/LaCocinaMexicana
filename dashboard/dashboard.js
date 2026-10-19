import '../style.css';
import './dashboard.css';
import { CATEGORIES, escapeHtml as e, renderMenu } from '../menu-renderer.js';

const app = document.querySelector('#app');
const state = { user: null, csrf: '', content: null, baseline: '', page: 'menu', selected: null, category: '', search: '', mobile: false, saving: false, uploading: false, conflict: false, users: [] };
const dirty = () => !!state.content && JSON.stringify(state.content) !== state.baseline;
const clone = (data) => structuredClone(data);
const options = (selected, all = false) => `${all ? '<option value="">All categories</option>' : ''}${CATEGORIES.map((c) => `<option ${c === selected ? 'selected' : ''}>${e(c)}</option>`).join('')}`;
const passwordField = (name, label, autocomplete = 'new-password', hint = true) => `<label>${label}<div class="password-control"><input type="password" name="${name}" autocomplete="${autocomplete}" required minlength="${autocomplete === 'current-password' ? 1 : 12}" maxlength="256"><button type="button" class="secondary" data-toggle-password="${name}" aria-label="Show ${label.toLowerCase()}">Show</button></div>${hint ? '<small class="password-hint">At least 12 characters. A long, unique passphrase works well.</small>' : ''}</label>`;

function notify(message, error = false) {
  const toast = document.createElement('div'); toast.className = `toast${error ? ' error' : ''}`;
  const text = document.createElement('span'); text.textContent = message;
  const close = document.createElement('button'); close.textContent = '×'; close.setAttribute('aria-label', 'Dismiss notification'); close.onclick = () => toast.remove();
  toast.append(text, close); document.querySelector('#notifications').append(toast);
  if (!error) setTimeout(() => toast.remove(), 8000);
}
function confirmAction(title, copy, action = 'Confirm') {
  const dialog = document.querySelector('#confirm-dialog');
  document.querySelector('#confirm-title').textContent = title; document.querySelector('#confirm-copy').textContent = copy;
  const ok = document.querySelector('#confirm-ok'); ok.textContent = action;
  return new Promise((resolve) => {
    const done = (value) => { dialog.close(); ok.onclick = null; document.querySelector('#confirm-cancel').onclick = null; dialog.oncancel = null; resolve(value); };
    ok.onclick = () => done(true); document.querySelector('#confirm-cancel').onclick = () => done(false);
    dialog.oncancel = (event) => { event.preventDefault(); done(false); }; dialog.showModal();
  });
}
async function request(path, { method = 'GET', body } = {}) {
  let response;
  try { response = await fetch(`/api${path}`, { method, credentials: 'same-origin', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': state.csrf }, body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(30000) }); }
  catch { throw new Error('Could not reach the server. Check your connection and try again. Your draft remains in this tab.'); }
  const data = await response.json().catch(() => ({ message: 'The server returned an unexpected response. Please try again.' }));
  if (!response.ok) {
    if (response.status === 401 && path !== '/auth/login') { state.previousUsername = state.user?.username; state.user = null; renderAuth('Please sign in again. Your draft will remain in this tab.'); }
    if (data.code === 'password_change_required' && state.user) { state.user.must_change_password = true; renderAuth(data.message); }
    const error = new Error(data.message || 'This action could not be completed.'); error.code = data.code; throw error;
  }
  return data;
}
function renderAuth(message = '') {
  const change = !!state.user?.must_change_password;
  app.innerHTML = `<main class="auth-layout"><section class="auth-brand"><img src="/images/LaCocinaLogo.png" alt="La Cocina Mexicana"><span class="eyebrow">A little care. A lot of flavor.</span><h1>Your kitchen.<br>Your menu.<br>Your way.</h1><p>A welcoming place to keep your menu fresh and share the moments that make La Cocina special.</p></section><section class="auth-content"><div class="auth-card"><span class="eyebrow">Business dashboard</span><h2>${change ? 'Make it yours.' : 'Welcome back.'}</h2><p>${change ? `Before you continue, replace the temporary password for ${e(state.user.username)} with a password only you know.` : 'Sign in to take care of your menu and gallery.'}</p><form id="${change ? 'first-password' : 'login'}">${change ? passwordField('current_password', 'Temporary password', 'current-password', false) + passwordField('new_password', 'New password') + passwordField('confirm_password', 'Confirm new password', 'new-password', false) : '<label>Login name<input name="username" autocomplete="username" required maxlength="100" spellcheck="false" autocapitalize="none" placeholder="Your login name"></label>' + passwordField('password', 'Password', 'current-password', false)}<div class="form-error" role="alert">${e(message)}</div><button type="submit">${change ? 'Save password & continue' : 'Sign in to dashboard'}</button></form>${change ? '<button type="button" class="quiet" data-action="logout">Sign out</button>' : '<p class="muted password-hint">Need a password reset? Ask a business owner to set a temporary password for you.</p>'}</div></section></main>`;
  app.querySelector('input')?.focus();
}
function navButton(page, symbol, label) { return `<button data-page="${page}" class="${state.page === page ? 'active' : ''}" ${state.page === page ? 'aria-current="page"' : ''}><span class="nav-symbol" aria-hidden="true">${symbol}</span>${label}</button>`; }
function renderShell() {
  const titles = { menu: 'Menu', gallery: 'Gallery', users: 'Team & access', account: 'My password' };
  app.innerHTML = `<div class="shell"><aside class="sidebar"><a class="brand" href="https://lacocinamexicana.net" target="_blank" rel="noopener"><img src="/images/LaCocinaLogo.png" alt=""><span>La Cocina<br>Mexicana</span></a><nav aria-label="Dashboard">${navButton('menu', '≡', 'Menu')}${navButton('gallery', '▧', 'Gallery')}${state.user.role === 'owner' ? navButton('users', '♧', 'Team & access') : ''}${navButton('account', '⚙', 'My account')}</nav><div class="sidebar-bottom"><a href="https://lacocinamexicana.net" target="_blank" rel="noopener">Visit website ↗</a><div class="account-name">${e(state.user.username)}</div><span class="role-pill">${e(state.user.role)}</span><button class="quiet" data-action="logout">Sign out ↗</button></div></aside><main class="main"><header class="topbar"><div class="breadcrumb">Your restaurant &nbsp;/&nbsp; <strong>${titles[state.page]}</strong><button class="quiet mobile-account" data-action="logout">Sign out</button></div><div class="publish-actions"><span id="save-state" class="save-state" role="status"></span><button class="secondary" id="discard" data-action="discard">Discard</button><button id="publish" data-action="publish">Publish changes ↗</button></div></header><div class="workspace" id="workspace"></div></main></div>`;
  renderPage(); updateSaveState();
}
function heading(eyebrow, title, text, button = '') { return `<div class="page-heading"><div><span class="eyebrow">${eyebrow}</span><h1>${title}</h1><p>${text}</p></div>${button}</div>`; }
function renderPage() {
  const workspace = app.querySelector('#workspace'); if (!workspace) return;
  if (state.page === 'menu') {
    workspace.innerHTML = `${heading('Made fresh, kept current', 'A menu to come back for.', 'Edit a dish below. See exactly how it looks before you publish.', '<button data-action="add-item">+ Add menu item</button>')}<div class="stat-strip"><div><strong id="item-count">${state.content.menu_items.length}</strong><span>menu items</span></div><div><strong>6</strong><span>categories</span></div><div><strong id="vegetarian-count">${state.content.menu_items.filter((i) => i.vegetarian).length}</strong><span>vegetarian options</span></div></div><div id="conflict-notice"></div><div class="editor-layout"><section class="panel"><div class="panel-header"><div><h2>Your dishes</h2><p>Select an item to make it your own</p></div><span class="role-pill">Menu editor</span></div><div class="filters"><label>Find a dish<input type="search" id="menu-search" placeholder="Search menu…" value="${e(state.search)}"></label><label>Category<select id="category-filter">${options(state.category, true)}</select></label></div><div class="item-list" id="item-list"></div><div class="item-editor" id="item-editor"></div></section><section class="panel preview-panel" aria-label="Live menu preview"><div class="panel-header"><div><h2>Live preview <span aria-hidden="true">↗</span></h2><p>Updates as you type</p></div><div class="preview-toolbar"><button class="secondary" data-view="desktop" aria-pressed="${!state.mobile}">Desktop</button><button class="secondary" data-view="mobile" aria-pressed="${state.mobile}">Mobile</button></div></div><p class="preview-caption">Draft preview · Customers see changes after you publish.</p><div class="preview-viewport"><div class="preview-page${state.mobile ? ' mobile' : ''}" id="preview-page"><p class="preview-kicker">La Cocina Mexicana</p><h2 class="preview-title">Our Menu</h2><div class="preview-rule"></div><div id="menu-preview"></div></div></div></section></div>`;
    renderList(); renderItemEditor(); updatePreview();
  } else if (state.page === 'gallery') {
    workspace.innerHTML = `${heading('Moments worth sharing', 'A taste of La Cocina.', 'Add photos, describe them, and arrange the story your guests see.')}<div id="conflict-notice"></div><section class="upload-zone"><div><h2>Bring something fresh to the gallery</h2><p>JPEG, PNG or WebP · Up to 8 MB · Photos are optimized automatically</p><p id="upload-status" role="status">Uploads stay in your draft until you publish.</p><progress id="upload-progress" max="100" value="0" hidden aria-label="Image upload progress"></progress></div><button data-action="choose-photo" ${state.uploading ? 'disabled' : ''}>${state.uploading ? 'Uploading…' : '+ Upload a photo'}</button><input id="photo-file" class="sr-only" type="file" accept="image/jpeg,image/png,image/webp" aria-label="Choose gallery photo"></section><div class="gallery-cards">${state.content.gallery.map((photo, i) => `<article class="photo-card"><img src="${e(photo.url)}" alt="${e(photo.alt)}"><div class="photo-body"><label>Image description<input data-photo-id="${e(photo.id)}" value="${e(photo.alt)}" maxlength="300" required placeholder="Describe the photo for your guests"></label><div class="photo-actions"><div><button class="secondary" data-move-photo="${e(photo.id)}" data-direction="-1" ${i === 0 ? 'disabled' : ''} aria-label="Move photo ${i + 1} earlier">←</button> <button class="secondary" data-move-photo="${e(photo.id)}" data-direction="1" ${i === state.content.gallery.length - 1 ? 'disabled' : ''} aria-label="Move photo ${i + 1} later">→</button></div><button class="quiet" data-replace-photo="${e(photo.id)}">Replace</button><button class="quiet" data-remove-photo="${e(photo.id)}">Remove</button></div></div></article>`).join('') || '<p class="empty-state">Your gallery is empty. Add the first photo above.</p>'}</div>`;
  } else if (state.page === 'users' && state.user.role === 'owner') {
    workspace.innerHTML = `${heading('Good food takes a team', 'The right people. The right access.', 'Owners manage accounts. Owners and admins can keep the menu and gallery up to date.')}<div class="user-layout"><section class="panel"><div class="panel-header"><h2>Your team</h2><button class="quiet" data-action="refresh-users">Refresh</button></div><div id="user-list"><p class="empty-state">Loading accounts…</p></div></section><section class="panel"><div class="panel-header"><h2>Add a new user</h2><span class="role-pill">Owners only</span></div><form id="create-user" class="panel-body"><label>Login name<input name="username" required minlength="3" maxlength="100" autocomplete="off" autocapitalize="none" spellcheck="false" placeholder="e.g. maria.cocina"></label><label>Role<select name="role"><option value="admin">Admin — edit menu & gallery</option><option value="owner">Owner — edit content & manage all accounts</option></select></label>${passwordField('password', 'Temporary password')}<p class="notice">Share this temporary password directly with the user. They must choose a new password before they can use the dashboard.</p><div class="form-error" role="alert"></div><button type="submit">Create account</button></form></section></div>`;
    loadUsers();
  } else {
    workspace.innerHTML = `${heading('A little housekeeping', 'Keep your account secure.', `Signed in as ${e(state.user.username)} · ${e(state.user.role)}`)}<section class="panel account-panel"><div class="panel-header"><h2>Change your password</h2></div><form id="change-password" class="panel-body">${passwordField('current_password', 'Current password', 'current-password', false)}${passwordField('new_password', 'New password')}${passwordField('confirm_password', 'Confirm new password', 'new-password', false)}<p class="notice">Changing your password signs out your other sessions.</p><div class="form-error" role="alert"></div><button type="submit">Update password</button></form></section>`;
  }
  renderConflict();
}
function renderList() {
  const list = app.querySelector('#item-list'); if (!list) return;
  const items = state.content.menu_items.filter((i) => (!state.category || i.category === state.category) && `${i.name} ${i.description}`.toLowerCase().includes(state.search.toLowerCase()));
  list.innerHTML = items.map((item) => `<button class="dish-row${state.selected === item.id ? ' selected' : ''}" data-select-item="${e(item.id)}" aria-pressed="${state.selected === item.id}"><span><strong>${e(item.name)}${item.vegetarian ? ' · V' : ''}</strong><small>${e(item.category)}</small></span><span>${e(item.price)}</span></button>`).join('') || '<p class="empty-state">No dishes match. Try another search or add a menu item.</p>';
}
function renderItemEditor() {
  const editor = app.querySelector('#item-editor'); if (!editor) return;
  const item = state.content.menu_items.find((i) => i.id === state.selected);
  if (!item) { editor.innerHTML = '<p class="muted">Select a dish above to edit its details.</p>'; return; }
  editor.innerHTML = `<h3>Edit dish</h3><form id="item-form"><label>Dish name<input name="name" value="${e(item.name)}" required maxlength="160"></label><div class="field-row"><label>Category<select name="category">${options(item.category)}</select></label><label>Price as shown on menu<input name="price" value="${e(item.price)}" required maxlength="64" placeholder="$14 or $18 Single / $35 Double"></label></div><label>Description<textarea name="description" maxlength="2000">${e(item.description)}</textarea></label><label><input name="vegetarian" type="checkbox" ${item.vegetarian ? 'checked' : ''}>Vegetarian option</label><div class="editor-footer"><small>Changes appear instantly in the preview.</small><button class="quiet" type="button" data-action="remove-item">Remove dish</button></div></form>`;
}
function focusPreview() {
  const viewport = app.querySelector('.preview-viewport');
  const dish = app.querySelector(`[data-menu-id="${CSS.escape(state.selected || '')}"]`);
  app.querySelectorAll('.preview-selected').forEach((item) => item.classList.remove('preview-selected'));
  if (!viewport || !dish) return;
  dish.classList.add('preview-selected');
  const top = dish.getBoundingClientRect().top - viewport.getBoundingClientRect().top + viewport.scrollTop;
  if (top < viewport.scrollTop || top + dish.offsetHeight > viewport.scrollTop + viewport.clientHeight) viewport.scrollTop = Math.max(0, top - 35);
}
function updatePreview() { const preview = app.querySelector('#menu-preview'); if (preview) { preview.innerHTML = renderMenu(state.content.menu_items); focusPreview(); } }
function updateSaveState() {
  const status = app.querySelector('#save-state'); if (!status) return;
  const hasChanges = dirty();
  status.className = `save-state${state.conflict ? ' error' : hasChanges ? ' dirty' : ''}`;
  status.textContent = state.saving ? 'Publishing…' : state.uploading ? 'Uploading photo…' : state.conflict ? 'Published version changed' : hasChanges ? 'Unpublished changes' : state.content.updated_at ? `Published ${new Date(state.content.updated_at * 1000).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' })}` : 'All changes published';
  app.querySelector('#publish').disabled = !hasChanges || state.saving || state.uploading || state.conflict;
  app.querySelector('#discard').disabled = !hasChanges || state.saving || state.uploading;
  if (app.querySelector('#item-count')) app.querySelector('#item-count').textContent = state.content.menu_items.length;
  if (app.querySelector('#vegetarian-count')) app.querySelector('#vegetarian-count').textContent = state.content.menu_items.filter((i) => i.vegetarian).length;
}
function renderConflict() {
  const notice = app.querySelector('#conflict-notice');
  if (notice) notice.innerHTML = state.conflict ? '<div class="notice" role="alert">Someone else published a newer version. Your draft is still visible. Download it for reference, then reload the published version before editing.<br><button class="secondary" data-action="download-draft">Download my draft</button> <button class="secondary" data-action="reload-content">Reload published version</button></div>' : '';
}
async function loadWorkspace() {
  const data = await request('/content');
  if (!dirty()) { state.content = data; state.baseline = JSON.stringify(data); }
  else if (state.content.revision !== data.revision) state.conflict = true;
  state.selected ||= state.content.menu_items[0]?.id; renderShell();
}
async function loadUsers() {
  try {
    const data = await request('/users'); state.users = data.users;
    const list = app.querySelector('#user-list'); if (!list) return;
    list.innerHTML = data.users.map((user) => `<article class="user-card"><div class="user-card-head"><strong>${e(user.username)}${user.id === state.user.id ? ' (you)' : ''}</strong><span class="role-pill">${e(user.role)}</span></div><p>${user.must_change_password ? '◷ Password setup required at next sign-in' : '✓ Account ready'}</p><details><summary>Reset password</summary><form data-reset-user="${user.id}">${passwordField(`reset_${user.id}`, 'New temporary password')}<small>All sessions will end immediately. This user must change the password at next sign-in.${user.id === state.user.id ? ' Resetting your own password signs you out.' : ''}</small><div class="form-error" role="alert"></div><button type="submit" class="secondary">Reset password</button></form></details></article>`).join('');
  } catch (error) { const list = app.querySelector('#user-list'); if (list) list.innerHTML = `<div class="panel-body"><p class="form-error" role="alert">${e(error.message)}</p><button data-action="refresh-users">Try again</button></div>`; }
}
function acceptSession(data) {
  if ((state.previousUsername || state.user?.username) !== data.user.username) { state.content = null; state.baseline = ''; state.conflict = false; state.selected = null; state.page = 'menu'; }
  state.user = data.user; state.previousUsername = data.user.username; state.csrf = data.csrf_token;
}
async function afterSignIn(data) {
  acceptSession(data);
  if (state.user.must_change_password) { renderAuth(); return; }
  try { await loadWorkspace(); } catch (error) { app.innerHTML = `<main class="auth-content"><div class="auth-card"><h2>Signed in.</h2><p class="form-error">${e(error.message)}</p><button data-action="retry-workspace">Retry loading dashboard</button><button class="quiet" data-action="logout">Sign out</button></div></main>`; }
}
async function publish() {
  if (!dirty() || state.saving || state.uploading || state.conflict) return;
  const invalidDish = state.content.menu_items.find((i) => !i.name.trim() || !i.price.trim());
  const invalidPhoto = state.content.gallery.find((p) => !p.alt.trim());
  if (invalidDish || invalidPhoto) {
    state.page = invalidDish ? 'menu' : 'gallery'; if (invalidDish) state.selected = invalidDish.id;
    renderShell(); notify(invalidDish ? 'Add a name and price for every dish before publishing.' : 'Add a description to every photo before publishing.', true); return;
  }
  state.saving = true; updateSaveState(); app.querySelector('#workspace').inert = true;
  try {
    const result = await request('/content', { method: 'PUT', body: state.content });
    state.content = result; state.baseline = JSON.stringify(result); state.conflict = false;
    notify('Published! Your menu and gallery are now live. Open website pages refresh within 30 seconds.');
  } catch (error) { if (error.code === 'content_conflict') { state.conflict = true; renderConflict(); } notify(error.message, true); }
  finally { state.saving = false; const workspace = app.querySelector('#workspace'); if (workspace) workspace.inert = false; updateSaveState(); }
}
async function uploadPhoto(file, replaceId) {
  if (!file || state.uploading) return;
  if (file.size > 8 * 1024 * 1024 || !['image/jpeg', 'image/png', 'image/webp'].includes(file.type)) { notify('Choose a JPEG, PNG or WebP image up to 8 MB.', true); return; }
  if (!replaceId && state.content.gallery.length >= 100) { notify('The gallery supports up to 100 photos. Remove a photo before adding another.', true); return; }
  state.uploading = true; const progress = app.querySelector('#upload-progress'); const status = app.querySelector('#upload-status');
  if (progress) progress.hidden = false;
  updateSaveState(); app.querySelector('[data-action="choose-photo"]')?.setAttribute('disabled', '');
  try {
    const result = await new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest(); xhr.open('POST', '/api/uploads'); xhr.setRequestHeader('X-CSRF-Token', state.csrf); xhr.timeout = 90000;
      xhr.upload.onprogress = (event) => { if (event.lengthComputable && progress) { progress.value = event.loaded / event.total * 100; if (status) status.textContent = progress.value === 100 ? 'Optimizing your photo…' : `Uploading… ${Math.round(progress.value)}%`; } };
      xhr.onload = () => { let result; try { result = JSON.parse(xhr.responseText); } catch { reject(new Error('Upload failed. Please try again.')); return; } if (xhr.status >= 200 && xhr.status < 300) resolve(result); else { if (xhr.status === 401) { state.previousUsername = state.user?.username; state.user = null; renderAuth('Your session ended. Sign in again to continue. Your draft stays in this tab.'); } reject(new Error(result.message || 'Upload failed. Please try again.')); } };
      xhr.onerror = xhr.ontimeout = () => reject(new Error('Upload interrupted. Check your connection and try again.'));
      const form = new FormData(); form.append('file', file); xhr.send(form);
    });
    const photo = state.content.gallery.find((p) => p.id === replaceId);
    if (photo) photo.url = result.url;
    else state.content.gallery.push({ id: crypto.randomUUID(), url: result.url, alt: '' });
    notify('Photo uploaded to your draft. Add a description, then publish.');
  } catch (error) { notify(error.message, true); }
  finally { state.uploading = false; if (state.user) { renderPage(); updateSaveState(); } }
}

app.addEventListener('input', (event) => {
  const target = event.target;
  if (target.id === 'menu-search') { state.search = target.value; renderList(); }
  if (target.id === 'category-filter') { state.category = target.value; renderList(); }
  if (target.closest('#item-form')) {
    const item = state.content.menu_items.find((i) => i.id === state.selected);
    if (item) { item[target.name] = target.type === 'checkbox' ? target.checked : target.value; renderList(); updatePreview(); updateSaveState(); }
  }
  if (target.dataset.photoId) { const photo = state.content.gallery.find((p) => p.id === target.dataset.photoId); photo.alt = target.value; target.closest('.photo-card').querySelector('img').alt = target.value; updateSaveState(); }
});
app.addEventListener('change', (event) => {
  if (event.target.id === 'photo-file') { const input = event.target; uploadPhoto(input.files[0], input.dataset.replaceId); input.value = ''; delete input.dataset.replaceId; }
});
app.addEventListener('click', async (event) => {
  const button = event.target.closest('button'); if (!button || button.disabled) return;
  if (button.dataset.togglePassword) { const input = button.closest('form').elements[button.dataset.togglePassword]; const show = input.type === 'password'; input.type = show ? 'text' : 'password'; button.textContent = show ? 'Hide' : 'Show'; button.setAttribute('aria-label', `${show ? 'Hide' : 'Show'} password`); return; }
  if (button.dataset.page) { if (state.saving || state.uploading) { notify('Please wait for the current action to finish.'); return; } state.page = button.dataset.page; renderShell(); return; }
  if (button.dataset.selectItem) { state.selected = button.dataset.selectItem; renderList(); renderItemEditor(); focusPreview(); return; }
  if (button.dataset.view) { state.mobile = button.dataset.view === 'mobile'; app.querySelector('#preview-page').classList.toggle('mobile', state.mobile); app.querySelectorAll('[data-view]').forEach((b) => b.setAttribute('aria-pressed', b.dataset.view === button.dataset.view)); return; }
  const action = button.dataset.action;
  try {
    if (action === 'publish') await publish();
    if (action === 'add-item') { if (state.content.menu_items.length >= 1000) { notify('The menu supports up to 1,000 items.', true); return; } const item = { id: crypto.randomUUID(), name: 'New dish', category: state.category || CATEGORIES[0], price: '', description: '', vegetarian: false }; state.content.menu_items.push(item); state.selected = item.id; state.search = ''; renderPage(); updateSaveState(); app.querySelector('#item-form input')?.focus(); }
    if (action === 'remove-item') { const item = state.content.menu_items.find((i) => i.id === state.selected); if (item && await confirmAction('Remove this dish?', `${item.name} will be removed from your draft. Customers will see the change after you publish.`, 'Remove dish')) { state.content.menu_items = state.content.menu_items.filter((i) => i.id !== item.id); state.selected = null; renderPage(); updateSaveState(); notify('Dish removed from draft. Publish when you are ready.'); } }
    if (action === 'discard' || action === 'reload-content') { if (await confirmAction('Reload published content?', 'Your unpublished menu and gallery edits will be discarded.', 'Reload published version')) { const data = await request('/content'); state.content = data; state.baseline = JSON.stringify(data); state.conflict = false; renderPage(); updateSaveState(); notify('Published content reloaded.'); } }
    if (action === 'download-draft') { const url = URL.createObjectURL(new Blob([JSON.stringify(state.content, null, 2)], { type: 'application/json' })); const link = document.createElement('a'); link.href = url; link.download = 'la-cocina-menu-draft.json'; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); }
    if (action === 'refresh-users') await loadUsers();
    if (action === 'retry-workspace') await loadWorkspace();
    if (action === 'choose-photo' || button.dataset.replacePhoto) { const input = app.querySelector('#photo-file'); input.dataset.replaceId = button.dataset.replacePhoto || ''; input.click(); }
    if (button.dataset.movePhoto) { const index = state.content.gallery.findIndex((p) => p.id === button.dataset.movePhoto); const to = index + Number(button.dataset.direction); [state.content.gallery[index], state.content.gallery[to]] = [state.content.gallery[to], state.content.gallery[index]]; renderPage(); updateSaveState(); document.querySelector('#announcer').textContent = `Photo moved to position ${to + 1}.`; }
    if (button.dataset.removePhoto && await confirmAction('Remove this photo?', 'It will leave your draft gallery. Publish to update the website.', 'Remove photo')) { state.content.gallery = state.content.gallery.filter((p) => p.id !== button.dataset.removePhoto); renderPage(); updateSaveState(); notify('Photo removed from draft.'); }
    if (action === 'logout') { if (state.uploading || state.saving) { notify('Please wait for the current action to finish.'); return; } if (dirty() && !await confirmAction('Sign out with unpublished edits?', 'Your draft will be discarded when you sign out.', 'Discard & sign out')) return; await request('/auth/logout', { method: 'POST' }); state.user = null; state.content = null; state.baseline = ''; state.csrf = ''; renderAuth(); notify('You have been signed out.'); }
  } catch (error) { notify(error.message, true); }
});
app.addEventListener('submit', async (event) => {
  event.preventDefault(); const form = event.target; if (form.id === 'item-form') return;
  if (form.dataset.busy) return;
  const fields = Object.fromEntries(new FormData(form)); const errorBox = form.querySelector('.form-error'); if (errorBox) errorBox.textContent = '';
  if ('confirm_password' in fields && fields.new_password !== fields.confirm_password) { errorBox.textContent = 'The new passwords do not match.'; form.elements.confirm_password.focus(); return; }
  if (form.dataset.resetUser) {
    const target = state.users.find((u) => u.id === Number(form.dataset.resetUser));
    if (!await confirmAction(`Reset ${target.username}'s password?`, 'This will end all their sessions and require them to choose a new password at their next sign-in.', 'Reset password')) return;
  }
  const submit = form.querySelector('[type=submit]'); const original = submit.textContent; submit.disabled = true; submit.textContent = 'Please wait…'; form.dataset.busy = 'true';
  try {
    if (form.id === 'login') { const data = await request('/auth/login', { method: 'POST', body: fields }); form.reset(); await afterSignIn(data); }
    else if (form.id === 'first-password' || form.id === 'change-password') { const first = form.id === 'first-password'; const data = await request('/auth/password', { method: 'POST', body: fields }); acceptSession(data); form.reset(); if (first) await afterSignIn(data); notify('Your password has been updated. You are ready to go.'); }
    else if (form.id === 'create-user') { const data = await request('/users', { method: 'POST', body: fields }); form.reset(); await loadUsers(); notify(data.message); }
    else if (form.dataset.resetUser) { const data = await request(`/users/${form.dataset.resetUser}/password`, { method: 'POST', body: { password: fields[`reset_${form.dataset.resetUser}`] } }); form.reset(); if (Number(form.dataset.resetUser) === state.user.id) { state.user = null; state.csrf = ''; renderAuth('Your password was reset. Sign in with the temporary password you just entered.'); } else await loadUsers(); notify(data.message); }
  } catch (error) { if (form.isConnected && errorBox) errorBox.textContent = error.message; else notify(error.message, true); }
  finally { submit.disabled = false; submit.textContent = original; delete form.dataset.busy; }
});
window.addEventListener('beforeunload', (event) => { if (dirty() || state.uploading || state.saving) { event.preventDefault(); event.returnValue = ''; } });
window.addEventListener('offline', () => notify('You are offline. Your draft stays in this tab; reconnect before publishing.', true));
window.addEventListener('online', () => notify('Connection restored. You can publish your changes.'));
// Detect another editor's publication without overwriting this tab's draft.
setInterval(async () => {
  if (!state.user || state.user.must_change_password || !state.content || state.saving || state.uploading || document.hidden) return;
  try { const data = await request('/content'); if (data.revision !== state.content.revision) { if (dirty()) { state.conflict = true; renderConflict(); updateSaveState(); } else { state.content = data; state.baseline = JSON.stringify(data); if (['menu', 'gallery'].includes(state.page)) renderPage(); updateSaveState(); notify('The latest published changes from your team are now visible.'); } } } catch { /* Foreground actions show actionable errors. */ }
}, 30000);

try { await afterSignIn(await request('/auth/me')); } catch { renderAuth(); }
