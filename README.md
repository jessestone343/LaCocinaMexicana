# LaCocinaMexicana

The restaurant site is built with Vite. The business dashboard is a separate,
persistent Rust service (Axum + SQLite) at `dashboard.lacocinamexicana.net`.
The service serves the dashboard, published content, and uploaded images. The
public site can remain on Vercel, including its existing Google reviews function.

## Business dashboard

- Owners and admins can add/remove menu items, edit names/descriptions/prices,
  choose categories, mark vegetarian options, and upload/replace/remove/reorder
  gallery images. Existing menu prices, including multiple sizes, are preserved.
- Edits update the menu preview immediately. The preview uses the **same renderer
  and menu stylesheet as the restaurant site**, with desktop/mobile views.
  The dish list and edit panel sit side by side on desktop; the preview moves
  below them on narrower desktop screens. On phones, selecting a dish brings
  the edit panel into view automatically.
- **Publish changes** makes the draft live atomically. The public site fetches
  the latest content on load and every 30 seconds while visible; no rebuild is
  needed. If the service is temporarily unavailable, an already open page keeps
  its last content; a new page falls back to the bundled original menu/gallery.
- Only owners see Team & access. The backend independently restricts listing,
  creating, and resetting accounts to owners. New accounts can be owners or admins.
- Owners can add, update, or remove the contact email for any owner/admin account
  in Team & access, including their own. My account also provides the signed-in
  owner's email editor. Email changes save immediately, with validation and
  duplicate-address feedback; they do not change login names or passwords.
  New accounts may include an optional contact email. Existing SQLite databases
  upgrade automatically on startup and retain all accounts, sessions, and content.
  Existing accounts initially have no contact email; an owner can add one.
- Every new or reset account must replace its temporary password before using
  protected dashboard features. Owners can enter a temporary password for **any**
  owner/admin account, including their own. Resets revoke all of that account's
  sessions immediately. Resetting your own password signs you out.
- Changes remain in memory in the current tab until published. Closing the tab or
  discarding loses that draft. Offline/error feedback preserves the current draft;
  publication conflicts offer a JSON download before reloading published content.
  A newer published version never silently overwrites an unpublished draft.

### Run locally

Requirements: Node 22.12+ (Node 24 recommended), Rust 1.93+, and a C compiler for
bundled SQLite (Visual Studio C++ Build Tools on Windows).

```powershell
npm ci
npm run build
# Choose your own unique temporary password; this is a local example only.
$env:BOOTSTRAP_PASSWORD = 'replace-with-your-own-long-temporary-passphrase'
$env:DASHBOARD_ORIGIN = 'http://localhost:5173'
$env:COOKIE_SECURE = 'false'
npm run backend
```

In another terminal, run `npm run dev`, then open
[the local dashboard](http://localhost:5173/dashboard/). The Vite proxy forwards
API requests/uploads to Rust on port 3000. Use `localhost` consistently; the
configured origin must exactly match the address in your browser. To serve the
production build directly from Rust locally, set `DASHBOARD_ORIGIN` to
`http://localhost:3000` instead and open port 3000. Rust serves the dashboard
directly at `/`; old `/dashboard` and `/dashboard/` links redirect to `/`.
Rust does not load `.env` files; export its settings explicitly.

Sign in with **`jesse.stone.truecore.services`**, exactly as requested (a login
name, not an email address), and the temporary password supplied at first
startup. The first screen requires your temporary password and a new password.
Passwords must contain at least 12 characters and at most 256 UTF-8 bytes.
There is no checked-in or shared default password and no public signup endpoint.

The first startup requires `BOOTSTRAP_PASSWORD` or `BOOTSTRAP_PASSWORD_FILE`.
Subsequent startups do not change accounts or reapply the bootstrap password.
Remove the bootstrap secret from the running environment after provisioning.
The first startup seeds all original dishes, prices, categories and gallery
photos. Later application deployments preserve database content.

### Deploy to dashboard.lacocinamexicana.net

The Rust service requires a long-running container/host and a persistent disk.
It cannot run as the existing Vercel JavaScript function. A complete Docker
build, persistent-volume Compose definition and Caddy reverse proxy example are
included in `Dockerfile`, `deploy/compose.yaml`, and `deploy/Caddyfile`.

1. On a Docker host, create `deploy/secrets/bootstrap_password.txt` containing
   the unique temporary password you choose (12+ characters). This path is
   ignored by Git and Docker build context. Allow UID 10001 to read the mounted
   secret; keep it inaccessible to other host users.
2. Run `docker compose -f deploy/compose.yaml up -d --build`. The named volume
   stores SQLite, its WAL, and uploaded images under `/data`. Do not delete it
   during upgrades. The service runs as an unprivileged user.
3. Point the `dashboard` DNS record at the host. Run Caddy on the host with the
   included configuration, with ports 80/443 reachable for automatic HTTPS.
   Only the reverse proxy should expose the service publicly; port 3000 binds
   to loopback. Alternatively use another TLS proxy or a persistent container
   host, forwarding the original dashboard host.
4. Keep `DASHBOARD_ORIGIN=https://dashboard.lacocinamexicana.net` and
   `COOKIE_SECURE=true`. Production startup rejects insecure origins when secure
   cookies are enabled. Insecure cookies are only supported on loopback for local
   development. `/health` returns readiness without credentials.
5. Deploy the public Vite build to Vercel as before. By default it reads published
   content from `https://dashboard.lacocinamexicana.net`. If hosting the service
   elsewhere, set public build variable `VITE_CONTENT_API_URL` to its origin.
   Cross-origin reads are allowed only for public content; dashboard APIs remain
   same-origin. Uploaded photos load from the content service. Seed images are
   included in both production builds.
6. Visit [the production dashboard](https://dashboard.lacocinamexicana.net), sign
   in as `jesse.stone.truecore.services` with your selected temporary password,
   and complete the mandatory password change. Then create other accounts.
7. Remove `BOOTSTRAP_PASSWORD_FILE` and the `secrets` entries from the Compose
   service after initial provisioning; remove the no-longer-needed secret file.
   Keep database backups available before removing the bootstrap secret.

No domain changes or deployment are performed by committing this project.

### Storage, access controls, and recovery

`DATA_DIR` defaults to `data/`; `STATIC_DIR` defaults to `dist/` relative to the
process working directory. Run from the repository root in development. Persist
the **entire** data directory. Use SQLite's online backup command/API (or stop the
service before copying the database and WAL), and also back up `uploads/`.
Test restoration before relying on backups. SQLite is intended for a single
service instance using a local persistent volume; do not horizontally replicate
instances with separate files or place the database on network storage.

Password hashes use Argon2id. Random session tokens are stored as SHA-256 hashes
server-side; cookies are HttpOnly, SameSite=Strict, Secure in production and
expire after eight hours. Password changes rotate sessions. Mutations require
the configured Origin and a per-session CSRF token. Login attempts are limited
to eight per login name per 15-minute window, plus a service-wide limit of 100;
these limits persist across restarts. Owner resets clear the target's login
counter. Public API responses have no cache. Account actions/publications have
an audit trail, and previous published content is retained in `content_history`.

Image uploads are limited to 8 MB, decoded as JPEG/PNG/WebP with size/allocation
limits, resized to at most 2400 × 2400, and re-encoded to WebP. Uploaded filenames
are generated by the server. SVG/HTML and external image URLs are rejected.
Uploaded images are public URLs as soon as uploaded, but appear in the gallery
only after publishing. Removed/replaced photos remain on disk for recovery and
history; plan storage cleanup alongside your retention policy. Alt descriptions
are required. Owner-created passwords are not retrievable after submission.

If everyone loses access, an operator with database access can replace an
account's `password_hash` with a freshly generated Argon2id PHC hash, set
`must_change_password=1`, delete that user's sessions and login-attempt counter,
and record the recovery operationally. Changing the bootstrap environment on an
existing database intentionally does **not** reset accounts. Prefer a second
owner account for routine recovery. Never delete production data to reset a
password.

Design references: [Axum uploads](https://docs.rs/axum/latest/axum/extract/struct.Multipart.html),
[RustCrypto Argon2](https://docs.rs/argon2/0.5.3/argon2/), and
[OWASP session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).

### Verification

```powershell
npm test
npm run build
cargo test --locked --manifest-path backend/Cargo.toml
cargo fmt --manifest-path backend/Cargo.toml -- --check
```

The API integration tests exercise mandatory password changes, role enforcement,
resetting both account types/self, revoked sessions, CSRF/Origin checks, publish
conflicts, validation, login throttling, persistence, real image uploads, owner-only
email updates for all account types/self, and upgrades of existing databases.

## Development

This site uses Vite so production builds automatically generate content-hashed CSS, JavaScript, and asset filenames. Do not add manual `?v=` cache-busting values to files.

- `npm run dev`: local development server.
- `npm run build`: production build to `dist/`.
- `npm run preview`: preview the production build locally.

Localhost automatically uses mock review data so the review grid can be reviewed without calling Google. Deployed sites always use the live Google reviews endpoint.

## Google reviews

The reviews section uses the Google Places API from the Vercel serverless function at `/api/google-reviews`.

Set these environment variables in Vercel:

- `GOOGLE_PLACES_API_KEY`: Google Maps Platform API key with Places API enabled.
- `GOOGLE_PLACE_ID`: Place ID for La Cocina Mexicana's Google Business Profile.
- `GOOGLE_REVIEWS_LIMIT`: Optional number of 5-star reviews to show, from `1` to `5`. Defaults to `3`.

Google's public Places response only returns a small set of place reviews, so the site filters that set down to 5-star reviews before display.

If reviews do not load after deployment, open `/api/google-reviews?debug=1` on the deployed site or add `?reviewsDebug=1` to the page URL. A Google `403` usually means the API key is restricted in a way that blocks server-side requests, billing is not active, or the enabled API does not match the endpoint. This project tries Places API (New) first and then the legacy Place Details endpoint.
