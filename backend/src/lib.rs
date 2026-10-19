use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use rand::{RngCore, rngs::OsRng};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
use tower_http::{
    limit::RequestBodyLimitLayer,
    services::{ServeDir, ServeFile},
};

const SESSION_SECONDS: i64 = 8 * 60 * 60;
const CATEGORIES: [&str; 6] = [
    "Starters",
    "Lunch Specials",
    "Entrées",
    "Especialidades",
    "Children's Menu",
    "Desserts",
];
type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub struct ApiError(StatusCode, &'static str, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"code":self.1,"message":self.2}))).into_response()
    }
}
fn bad(message: &'static str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, "validation", message)
}
fn internal(error: impl std::fmt::Display) -> ApiError {
    tracing::error!(%error, "Request failed");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "Something went wrong. Your changes were not confirmed. Please try again.",
    )
}
impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        internal(e)
    }
}
impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        internal(e)
    }
}

#[derive(Clone)]
pub struct Config {
    pub listen: String,
    pub origin: String,
    pub secure_cookies: bool,
    pub data_dir: PathBuf,
    pub static_dir: PathBuf,
    pub bootstrap_username: String,
    pub bootstrap_password: Option<String>,
}
impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let password = if let Ok(path) = std::env::var("BOOTSTRAP_PASSWORD_FILE") {
            Some(
                std::fs::read_to_string(path)?
                    .trim_end_matches(['\r', '\n'])
                    .to_string(),
            )
        } else {
            std::env::var("BOOTSTRAP_PASSWORD").ok()
        };
        let secure_cookies = std::env::var("COOKIE_SECURE")
            .unwrap_or_else(|_| "true".into())
            .parse::<bool>()?;
        let origin = std::env::var("DASHBOARD_ORIGIN")
            .unwrap_or_else(|_| "https://dashboard.lacocinamexicana.net".into());
        let uri: axum::http::Uri = origin.parse()?;
        if origin.ends_with('/')
            || uri.authority().is_none()
            || !matches!(uri.scheme_str(), Some("https" | "http"))
            || uri.path() != "/"
            || uri.query().is_some()
        {
            return Err(
                "DASHBOARD_ORIGIN must contain only scheme and host (no path or trailing slash)"
                    .into(),
            );
        }
        if secure_cookies && uri.scheme_str() != Some("https") {
            return Err("Secure cookies require an HTTPS dashboard origin".into());
        }
        if !secure_cookies && !matches!(uri.host(), Some("localhost" | "127.0.0.1" | "[::1]")) {
            return Err(
                "Insecure cookies are only allowed on loopback for local development".into(),
            );
        }
        Ok(Self {
            listen: std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into()),
            origin,
            secure_cookies,
            data_dir: std::env::var("DATA_DIR")
                .unwrap_or_else(|_| "data".into())
                .into(),
            static_dir: std::env::var("STATIC_DIR")
                .unwrap_or_else(|_| "dist".into())
                .into(),
            bootstrap_username: std::env::var("BOOTSTRAP_USERNAME")
                .unwrap_or_else(|_| "jesse.stone.truecore.services".into()),
            bootstrap_password: password,
        })
    }
}

pub struct AppState {
    db: Mutex<Connection>,
    config: Config,
    hashes: Arc<Semaphore>,
    dummy_hash: String,
}
type Shared = Arc<AppState>;
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn random() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}
fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
fn password_valid(password: &str) -> ApiResult<()> {
    if password.chars().count() < 12 || password.len() > 256 || password.trim().is_empty() {
        return Err(bad(
            "Use a password of at least 12 characters and at most 256 bytes.",
        ));
    }
    Ok(())
}
fn hash_sync(password: &str) -> ApiResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(internal)
}
async fn hash_password(state: &Shared, password: String) -> ApiResult<String> {
    password_valid(&password)?;
    let permit = state
        .hashes
        .clone()
        .acquire_owned()
        .await
        .map_err(internal)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_sync(&password)
    })
    .await
    .map_err(internal)?
}
async fn verify_password(state: &Shared, password: String, hash: String) -> ApiResult<bool> {
    if password.len() > 256 {
        return Ok(false);
    }
    let permit = state
        .hashes
        .clone()
        .acquire_owned()
        .await
        .map_err(internal)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&hash).is_ok_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .map_err(internal)
}
fn username(value: &str) -> ApiResult<String> {
    let result = value.trim().to_lowercase();
    if !(3..=100).contains(&result.len())
        || !result
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._@+-".contains(&b))
    {
        return Err(bad(
            "Login names must be 3–100 letters, numbers, dots, @, +, - or underscores.",
        ));
    }
    Ok(result)
}

fn email_address(value: &str) -> ApiResult<Option<String>> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok(None);
    }
    let invalid = || {
        bad(
            "Enter a valid email address, such as name@example.com, or leave it blank to remove it.",
        )
    };
    let (local, domain) = value.split_once('@').ok_or_else(invalid)?;
    if value.len() > 254
        || local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b))
        || !domain.contains('.')
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(invalid());
    }
    Ok(Some(value))
}

fn email_available(db: &Connection, email: &Option<String>, except_id: i64) -> ApiResult<()> {
    let taken: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE email=?1 COLLATE NOCASE AND id!=?2)",
        params![email, except_id],
        |r| r.get(0),
    )?;
    if taken {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "email_taken",
            "That email address is already associated with another account.",
        ));
    }
    Ok(())
}

pub async fn initialize(config: Config) -> Result<Shared, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(config.data_dir.join("uploads"))?;
    let mut db = Connection::open(config.data_dir.join("cocina.sqlite3"))?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute_batch(include_str!("schema.sql"))?;
    let tx = db.transaction()?;
    // Existing databases predate contact emails. Upgrade in place without
    // recreating accounts, changing login names, or revoking active sessions.
    let has_email = {
        let mut columns = tx.prepare("PRAGMA table_info(users)")?;
        let names = columns
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        names.iter().any(|name| name == "email")
    };
    if !has_email {
        tx.execute("ALTER TABLE users ADD COLUMN email TEXT COLLATE NOCASE", [])?;
    }
    tx.execute("CREATE UNIQUE INDEX IF NOT EXISTS users_email ON users(email COLLATE NOCASE) WHERE email IS NOT NULL", [])?;
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    if count == 0 {
        let password = config.bootstrap_password.as_ref().ok_or("First startup requires BOOTSTRAP_PASSWORD or BOOTSTRAP_PASSWORD_FILE (12+ characters). No default password is provided.")?;
        password_valid(password)
            .map_err(|_| "Bootstrap password must be 12–256 characters/bytes")?;
        let name =
            username(&config.bootstrap_username).map_err(|_| "Invalid bootstrap username")?;
        let hash = hash_sync(password).map_err(|_| "Unable to hash bootstrap password")?;
        tx.execute("INSERT INTO users(username,role,password_hash,must_change_password,created_at) VALUES (?1,'owner',?2,1,?3)", params![name,hash,now()])?;
    }
    tx.execute(
        "INSERT OR IGNORE INTO content(id,revision,body,updated_at) VALUES(1,1,?1,0)",
        [include_str!("../../content.seed.json")],
    )?;
    tx.commit()?;
    let dummy_hash = hash_sync(&random()).map_err(|_| "Unable to initialize password verifier")?;
    // Bootstrap secrets are only needed once; discard them from application state.
    let config = Config {
        bootstrap_password: None,
        ..config
    };
    Ok(Arc::new(AppState {
        db: Mutex::new(db),
        config,
        hashes: Arc::new(Semaphore::new(4)),
        dummy_hash,
    }))
}

#[derive(Clone, Serialize)]
pub struct User {
    id: i64,
    username: String,
    role: String,
    must_change_password: bool,
    email: Option<String>,
}
fn user_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        role: row.get(2)?,
        must_change_password: row.get(3)?,
        email: row.get(4)?,
    })
}
struct Auth {
    user: User,
    session_hash: String,
    csrf: String,
}
fn authenticate(
    db: &Connection,
    headers: &HeaderMap,
    ready: bool,
    mutation: bool,
) -> ApiResult<Auth> {
    let unauth = || {
        ApiError(
            StatusCode::UNAUTHORIZED,
            "session_expired",
            "Please sign in again. Your session has ended.",
        )
    };
    let token = headers
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|part| part.trim().strip_prefix("cocina_session="))
        })
        .ok_or_else(unauth)?;
    if token.len() != 64 {
        return Err(unauth());
    }
    let session_hash = digest(token);
    let result = db.query_row("SELECT u.id,u.username,u.role,u.must_change_password,u.email,s.csrf FROM sessions s JOIN users u ON s.user_id=u.id WHERE s.token_hash=?1 AND s.expires_at>?2", params![session_hash,now()], |row| Ok((user_row(row)?, row.get::<_,String>(5)?))).optional()?;
    let (user, csrf) = result.ok_or_else(unauth)?;
    if ready && user.must_change_password {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "password_change_required",
            "Create your new password before using the dashboard.",
        ));
    }
    if mutation && headers.get("x-csrf-token").and_then(|v| v.to_str().ok()) != Some(csrf.as_str())
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "csrf",
            "This request could not be verified. Refresh and try again.",
        ));
    }
    Ok(Auth {
        user,
        session_hash,
        csrf,
    })
}
fn owner(auth: &Auth) -> ApiResult<()> {
    if auth.user.role != "owner" {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "owner_required",
            "Only an owner can manage accounts.",
        ));
    }
    Ok(())
}
fn cookie(config: &Config, token: &str, age: i64) -> String {
    format!(
        "cocina_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={age}{}",
        if config.secure_cookies {
            "; Secure"
        } else {
            ""
        }
    )
}
fn session_response(db: &Connection, config: &Config, user: User) -> ApiResult<Response> {
    let token = random();
    let csrf = random();
    db.execute("DELETE FROM sessions WHERE expires_at<=?1", [now()])?;
    db.execute(
        "INSERT INTO sessions(token_hash,user_id,csrf,expires_at) VALUES(?1,?2,?3,?4)",
        params![digest(&token), user.id, csrf, now() + SESSION_SECONDS],
    )?;
    Ok((
        [(header::SET_COOKIE, cookie(config, &token, SESSION_SECONDS))],
        Json(json!({"user":user,"csrf_token":csrf})),
    )
        .into_response())
}
fn audit(db: &Connection, actor: i64, action: &str, target: &str) -> ApiResult<()> {
    db.execute(
        "INSERT INTO audit(actor_id,action,target,created_at) VALUES(?1,?2,?3,?4)",
        params![actor, action, target, now()],
    )?;
    Ok(())
}

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}
async fn login(State(state): State<Shared>, Json(input): Json<Login>) -> ApiResult<Response> {
    let name = username(&input.username).unwrap_or_default();
    let record = {
        let db = state.db.lock().map_err(internal)?;
        db.execute(
            "DELETE FROM login_attempts WHERE window_start<?1",
            [now() - 900],
        )?;
        for key in [format!("user:{name}"), "global".into()] {
            db.execute("INSERT INTO login_attempts(key,attempts,window_start) VALUES(?1,1,?2) ON CONFLICT(key) DO UPDATE SET attempts=attempts+1", params![key,now()])?;
            let attempts: i64 = db.query_row(
                "SELECT attempts FROM login_attempts WHERE key=?1",
                [&key],
                |r| r.get(0),
            )?;
            if attempts > if key == "global" { 100 } else { 8 } {
                return Err(ApiError(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    "Too many sign-in attempts. Please wait 15 minutes before trying again.",
                ));
            }
        }
        db.query_row("SELECT id,username,role,must_change_password,email,password_hash FROM users WHERE username=?1",[&name],|r| Ok((user_row(r)?,r.get::<_,String>(5)?))).optional()?
    };
    let hash = record
        .as_ref()
        .map(|r| r.1.clone())
        .unwrap_or_else(|| state.dummy_hash.clone());
    let verified = verify_password(&state, input.password, hash.clone()).await?;
    let Some((user, _)) = record.filter(|_| verified) else {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "The login name or password is incorrect.",
        ));
    };
    let db = state.db.lock().map_err(internal)?;
    let unchanged: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id=?1 AND password_hash=?2 AND username=?3)",
        params![user.id, hash, name],
        |r| r.get(0),
    )?;
    if !unchanged {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "Your login details changed. Please sign in again.",
        ));
    }
    db.execute(
        "DELETE FROM login_attempts WHERE key=?1",
        [format!("user:{name}")],
    )?;
    session_response(&db, &state.config, user)
}
async fn me(State(state): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let db = state.db.lock().map_err(internal)?;
    let auth = authenticate(&db, &headers, false, false)?;
    Ok(Json(json!({"user":auth.user,"csrf_token":auth.csrf})))
}
async fn logout(State(state): State<Shared>, headers: HeaderMap) -> ApiResult<Response> {
    let db = state.db.lock().map_err(internal)?;
    let auth = authenticate(&db, &headers, false, true)?;
    db.execute(
        "DELETE FROM sessions WHERE token_hash=?1",
        [auth.session_hash],
    )?;
    Ok((
        [(header::SET_COOKIE, cookie(&state.config, "", 0))],
        Json(json!({"message":"Signed out."})),
    )
        .into_response())
}
#[derive(Deserialize)]
struct PasswordChange {
    current_password: String,
    new_password: String,
}
async fn change_password(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(input): Json<PasswordChange>,
) -> ApiResult<Response> {
    let (id, old_hash) = {
        let db = state.db.lock().map_err(internal)?;
        let auth = authenticate(&db, &headers, false, true)?;
        let hash: String = db.query_row(
            "SELECT password_hash FROM users WHERE id=?1",
            [auth.user.id],
            |r| r.get(0),
        )?;
        (auth.user.id, hash)
    };
    if !verify_password(&state, input.current_password, old_hash.clone()).await? {
        return Err(bad("The current password is incorrect."));
    }
    if verify_password(&state, input.new_password.clone(), old_hash.clone()).await? {
        return Err(bad(
            "Choose a new password different from your current password.",
        ));
    }
    let hash = hash_password(&state, input.new_password).await?;
    let mut db = state.db.lock().map_err(internal)?;
    let tx = db.transaction()?;
    let mut auth = authenticate(&tx, &headers, false, true)?;
    if tx.execute(
        "UPDATE users SET password_hash=?1,must_change_password=0 WHERE id=?2 AND password_hash=?3",
        params![hash, id, old_hash],
    )? != 1
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "password_changed",
            "Your password changed. Please sign in again.",
        ));
    }
    tx.execute("DELETE FROM sessions WHERE user_id=?1", [id])?;
    audit(&tx, id, "password_changed", &id.to_string())?;
    auth.user.must_change_password = false;
    let response = session_response(&tx, &state.config, auth.user)?;
    tx.commit()?;
    Ok(response)
}
async fn users(State(state): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let db = state.db.lock().map_err(internal)?;
    let auth = authenticate(&db, &headers, true, false)?;
    owner(&auth)?;
    let mut stmt = db.prepare(
        "SELECT id,username,role,must_change_password,email FROM users ORDER BY username",
    )?;
    let users = stmt
        .query_map([], user_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({"users":users})))
}
#[derive(Deserialize)]
struct NewUser {
    username: String,
    role: String,
    password: String,
    #[serde(default)]
    email: Option<String>,
}
async fn create_user(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(input): Json<NewUser>,
) -> ApiResult<impl IntoResponse> {
    {
        let db = state.db.lock().map_err(internal)?;
        owner(&authenticate(&db, &headers, true, true)?)?;
    }
    let name = username(&input.username)?;
    let email = email_address(input.email.as_deref().unwrap_or(""))?;
    if !matches!(input.role.as_str(), "owner" | "admin") {
        return Err(bad("Choose an owner or admin role."));
    }
    let hash = hash_password(&state, input.password).await?;
    let mut db = state.db.lock().map_err(internal)?;
    let tx = db.transaction()?;
    let auth = authenticate(&tx, &headers, true, true)?;
    owner(&auth)?;
    if tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE username=?1)",
        [&name],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "username_taken",
            "That login name is already in use.",
        ));
    }
    email_available(&tx, &email, 0)?;
    tx.execute("INSERT INTO users(username,role,password_hash,must_change_password,created_at,email) VALUES(?1,?2,?3,1,?4,?5)",params![name,input.role,hash,now(),email])?;
    audit(&tx, auth.user.id, "user_created", &name)?;
    tx.commit()?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"message":"Account created. They must change their temporary password at first sign-in."}),
        ),
    ))
}

#[derive(Deserialize)]
struct UsernameUpdate {
    username: String,
}
async fn update_username(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<UsernameUpdate>,
) -> ApiResult<Json<Value>> {
    let mut db = state.db.lock().map_err(internal)?;
    let tx = db.transaction()?;
    let auth = authenticate(&tx, &headers, true, true)?;
    owner(&auth)?;
    let mut user = tx
        .query_row(
            "SELECT id,username,role,must_change_password,email FROM users WHERE id=?1",
            [id],
            user_row,
        )
        .optional()?
        .ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "not_found",
            "This account no longer exists.",
        ))?;
    let name = username(&input.username)?;
    if tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE username=?1 AND id<>?2)",
        params![name, id],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "username_taken",
            "That login name is already in use.",
        ));
    }
    if user.username != name {
        tx.execute(
            "UPDATE users SET username=?1 WHERE id=?2",
            params![name, id],
        )?;
        tx.execute(
            "DELETE FROM login_attempts WHERE key IN (?1,?2)",
            params![format!("user:{}", user.username), format!("user:{name}")],
        )?;
        audit(&tx, auth.user.id, "username_updated", &id.to_string())?;
        user.username = name;
    }
    tx.commit()?;
    Ok(Json(
        json!({"user":user,"message":"Login name saved. Use the new name the next time you sign in."}),
    ))
}

#[derive(Deserialize)]
struct EmailUpdate {
    email: String,
}
async fn update_email(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<EmailUpdate>,
) -> ApiResult<Json<Value>> {
    let mut db = state.db.lock().map_err(internal)?;
    let tx = db.transaction()?;
    let auth = authenticate(&tx, &headers, true, true)?;
    owner(&auth)?;
    let mut user = tx
        .query_row(
            "SELECT id,username,role,must_change_password,email FROM users WHERE id=?1",
            [id],
            user_row,
        )
        .optional()?
        .ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "not_found",
            "This account no longer exists.",
        ))?;
    let email = email_address(&input.email)?;
    email_available(&tx, &email, id)?;
    tx.execute("UPDATE users SET email=?1 WHERE id=?2", params![email, id])?;
    audit(&tx, auth.user.id, "email_updated", &id.to_string())?;
    let message = if email.is_some() {
        "Email address saved."
    } else {
        "Email address removed."
    };
    user.email = email;
    tx.commit()?;
    Ok(Json(json!({"user":user,"message":message})))
}

#[derive(Deserialize)]
struct Reset {
    password: String,
}
async fn reset_password(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<Reset>,
) -> ApiResult<Json<Value>> {
    {
        let db = state.db.lock().map_err(internal)?;
        owner(&authenticate(&db, &headers, true, true)?)?;
    }
    let hash = hash_password(&state, input.password).await?;
    let mut db = state.db.lock().map_err(internal)?;
    let tx = db.transaction()?;
    let auth = authenticate(&tx, &headers, true, true)?;
    owner(&auth)?;
    if tx.execute(
        "UPDATE users SET password_hash=?1,must_change_password=1 WHERE id=?2",
        params![hash, id],
    )? == 0
    {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "not_found",
            "This account no longer exists.",
        ));
    }
    tx.execute("DELETE FROM sessions WHERE user_id=?1", [id])?;
    tx.execute(
        "DELETE FROM login_attempts WHERE key=(SELECT 'user:' || username FROM users WHERE id=?1)",
        [id],
    )?;
    audit(&tx, auth.user.id, "password_reset", &id.to_string())?;
    tx.commit()?;
    Ok(Json(
        json!({"message":"Password reset. All existing sessions ended. A new password is required at next sign-in."}),
    ))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub price: String,
    pub category: String,
    pub vegetarian: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct GalleryImage {
    pub id: String,
    pub url: String,
    pub alt: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Content {
    pub revision: i64,
    pub updated_at: i64,
    pub menu_items: Vec<MenuItem>,
    pub gallery: Vec<GalleryImage>,
}
fn load_content(db: &Connection) -> ApiResult<Content> {
    let (body, revision, updated_at): (String, i64, i64) = db.query_row(
        "SELECT body,revision,updated_at FROM content WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut content: Content = serde_json::from_str(&body).map_err(internal)?;
    content.revision = revision;
    content.updated_at = updated_at;
    Ok(content)
}
async fn public_content(State(state): State<Shared>) -> ApiResult<Response> {
    let db = state.db.lock().map_err(internal)?;
    Ok((
        [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
        Json(load_content(&db)?),
    )
        .into_response())
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn validate_content(content: &Content, config: &Config) -> ApiResult<()> {
    if content.menu_items.len() > 1000 || content.gallery.len() > 100 {
        return Err(bad("Use at most 1,000 menu items and 100 gallery images."));
    }
    let mut ids = HashSet::new();
    for item in &content.menu_items {
        if !valid_id(&item.id) || !ids.insert(item.id.clone()) {
            return Err(bad("Menu item IDs must be unique."));
        }
        if item.name.trim().is_empty()
            || item.name.len() > 160
            || item.description.len() > 2000
            || item.price.trim().is_empty()
            || item.price.len() > 64
            || !CATEGORIES.contains(&item.category.as_str())
        {
            return Err(bad(
                "Each dish needs a name, a price, a valid category, and text within the allowed lengths.",
            ));
        }
    }
    ids.clear();
    let seed: Content =
        serde_json::from_str(include_str!("../../content.seed.json")).map_err(internal)?;
    for photo in &content.gallery {
        if !valid_id(&photo.id)
            || !ids.insert(photo.id.clone())
            || photo.alt.trim().is_empty()
            || photo.alt.len() > 300
        {
            return Err(bad(
                "Each gallery photo needs a unique ID and a description of up to 300 bytes.",
            ));
        }
        let seed_url = seed.gallery.iter().any(|p| p.url == photo.url);
        let uploaded = photo.url.strip_prefix("/uploads/").is_some_and(|file| {
            file.strip_suffix(".webp")
                .is_some_and(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                && config.data_dir.join("uploads").join(file).is_file()
        });
        if !seed_url && !uploaded {
            return Err(bad(
                "Use an existing gallery photo or upload a new image before publishing.",
            ));
        }
    }
    Ok(())
}
async fn publish(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(mut input): Json<Content>,
) -> ApiResult<Json<Content>> {
    let mut db = state.db.lock().map_err(internal)?;
    let tx = db.transaction()?;
    let auth = authenticate(&tx, &headers, true, true)?;
    validate_content(&input, &state.config)?;
    let current = load_content(&tx)?;
    if input.revision != current.revision {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "content_conflict",
            "Someone else published changes. Your draft is still here. Reload the published version before editing again.",
        ));
    }
    input.revision += 1;
    input.updated_at = now();
    let body = serde_json::to_string(&input).map_err(internal)?;
    tx.execute(
        "INSERT INTO content_history(revision,body,actor_id,created_at) VALUES(?1,?2,?3,?4)",
        params![
            current.revision,
            serde_json::to_string(&current).map_err(internal)?,
            auth.user.id,
            now()
        ],
    )?;
    tx.execute(
        "UPDATE content SET revision=?1,body=?2,updated_at=?3 WHERE id=1",
        params![input.revision, body, input.updated_at],
    )?;
    audit(
        &tx,
        auth.user.id,
        "content_published",
        &input.revision.to_string(),
    )?;
    tx.commit()?;
    Ok(Json(input))
}
async fn upload(
    State(state): State<Shared>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<impl IntoResponse> {
    {
        let db = state.db.lock().map_err(internal)?;
        authenticate(&db, &headers, true, true)?;
    }
    let field = multipart
        .next_field()
        .await
        .map_err(|_| bad("Choose a JPEG, PNG or WebP image up to 8 MB."))?
        .ok_or_else(|| bad("Choose an image to upload."))?;
    if field.name() != Some("file") {
        return Err(bad("Upload the image in the file field."));
    }
    let bytes = field
        .bytes()
        .await
        .map_err(|_| bad("The upload could not be read. Choose an image up to 8 MB."))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(bad("This image is too large. Choose one up to 8 MB."));
    }
    let permit = state
        .hashes
        .clone()
        .acquire_owned()
        .await
        .map_err(internal)?;
    let encoded = tokio::task::spawn_blocking(move || -> ApiResult<Vec<u8>> {
        let _permit = permit;
        let format =
            image::guess_format(&bytes).map_err(|_| bad("This file is not a supported image."))?;
        if !matches!(
            format,
            image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP
        ) {
            return Err(bad("Use JPEG, PNG or WebP images."));
        }
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8000);
        limits.max_image_height = Some(8000);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let img = reader.decode().map_err(|_| {
            bad("The image could not be opened. Use an image below 8,000 × 8,000 pixels.")
        })?;
        let img = img.resize(2400, 2400, image::imageops::FilterType::Lanczos3);
        let mut output = std::io::Cursor::new(Vec::new());
        img.write_to(&mut output, image::ImageFormat::WebP)
            .map_err(internal)?;
        Ok(output.into_inner())
    })
    .await
    .map_err(internal)??;
    // Re-check after decoding in case the user's password was reset during the upload.
    {
        let db = state.db.lock().map_err(internal)?;
        authenticate(&db, &headers, true, true)?;
    }
    let file = format!("{}.webp", random());
    tokio::fs::write(state.config.data_dir.join("uploads").join(&file), encoded).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"url":format!("/uploads/{file}"),"message":"Image uploaded. Add a description, then publish to show it on the website."}),
        ),
    ))
}

async fn security(State(state): State<Shared>, request: Request<Body>, next: Next) -> Response {
    let is_api = request.uri().path().starts_with("/api/");
    if is_api
        && !matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS
        )
        && request
            .headers()
            .get(header::ORIGIN)
            .and_then(|h| h.to_str().ok())
            != Some(state.config.origin.as_str())
    {
        return ApiError(
            StatusCode::FORBIDDEN,
            "origin",
            "Open the dashboard at its configured address and try again.",
        )
        .into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    if is_api {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    } else {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"));
    if state.config.secure_cookies {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}
pub fn app(state: Shared) -> Router {
    let static_dir = state.config.static_dir.clone();
    Router::new()
        .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/api/auth/login", post(login))
        .route("/api/auth/me", get(me))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", post(change_password))
        .route("/api/users", get(users).post(create_user))
        .route(
            "/api/users/{id}/username",
            axum::routing::put(update_username),
        )
        .route("/api/users/{id}/email", axum::routing::put(update_email))
        .route("/api/users/{id}/password", post(reset_password))
        .route("/api/content", get(public_content).put(publish))
        .route(
            "/api/uploads",
            post(upload).layer(DefaultBodyLimit::max(9 * 1024 * 1024)),
        )
        .route(
            "/api/{*path}",
            get(|| async {
                ApiError(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "This endpoint does not exist.",
                )
            }),
        )
        .route_service("/", ServeFile::new(static_dir.join("dashboard/index.html")))
        .route("/dashboard", get(|| async { Redirect::permanent("/") }))
        .route("/dashboard/", get(|| async { Redirect::permanent("/") }))
        .nest_service(
            "/uploads",
            ServeDir::new(state.config.data_dir.join("uploads")),
        )
        .fallback_service(
            ServeDir::new(&static_dir)
                .not_found_service(ServeFile::new(static_dir.join("404.html"))),
        )
        .layer(DefaultBodyLimit::max(512 * 1024))
        .layer(RequestBodyLimitLayer::new(9 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), security))
        .with_state(state)
}
