use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use cocina_dashboard::{Config, app, initialize};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const INITIAL: &str = "temporary-passphrase-123";
const CHANGED: &str = "my-new-long-passphrase-456";
const ORIGIN: &str = "http://localhost:5173";
struct Client {
    router: Router,
    cookie: String,
    csrf: String,
}
impl Client {
    async fn request(&mut self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("origin", ORIGIN)
            .header("content-type", "application/json")
            .header("cookie", &self.cookie)
            .header("x-csrf-token", &self.csrf)
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        if let Some(cookie) = response.headers().get("set-cookie") {
            self.cookie = cookie.to_str().unwrap().split(';').next().unwrap().into();
        }
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(csrf) = data["csrf_token"].as_str() {
            self.csrf = csrf.into();
        }
        (status, data)
    }
    async fn login(&mut self, name: &str, password: &str) -> (StatusCode, Value) {
        self.request(
            "POST",
            "/api/auth/login",
            json!({"username":name,"password":password}),
        )
        .await
    }
    async fn change(&mut self, current: &str, new: &str) -> (StatusCode, Value) {
        self.request(
            "POST",
            "/api/auth/password",
            json!({"current_password":current,"new_password":new}),
        )
        .await
    }
}
fn config(dir: &TempDir) -> Config {
    Config {
        listen: "127.0.0.1:0".into(),
        origin: ORIGIN.into(),
        secure_cookies: false,
        data_dir: dir.path().into(),
        static_dir: dir.path().join("dist"),
        bootstrap_username: "jesse.stone.truecore.services".into(),
        bootstrap_password: Some(INITIAL.into()),
    }
}
async fn setup() -> (TempDir, Client) {
    let dir = TempDir::new().unwrap();
    let router = app(initialize(config(&dir)).await.unwrap());
    (
        dir,
        Client {
            router,
            cookie: String::new(),
            csrf: String::new(),
        },
    )
}
async fn owner_ready(client: &mut Client) {
    assert_eq!(
        client
            .login("jesse.stone.truecore.services", INITIAL)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(client.change(INITIAL, CHANGED).await.0, StatusCode::OK);
}

#[tokio::test]
async fn temporary_password_is_gated_server_side_and_sessions_rotate() {
    let (_dir, mut client) = setup().await;
    let (_, session) = client.login("jesse.stone.truecore.services", INITIAL).await;
    assert_eq!(session["user"]["must_change_password"], true);
    assert!(!session.to_string().contains("password_hash"));
    let old_cookie = client.cookie.clone();
    let old_csrf = client.csrf.clone();
    assert_eq!(
        client.request("GET", "/api/users", json!(null)).await.0,
        StatusCode::FORBIDDEN
    );
    let (_, content) = client.request("GET", "/api/content", json!(null)).await;
    assert_eq!(
        client.request("PUT", "/api/content", content).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client.change(INITIAL, INITIAL).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        client.change(INITIAL, "short").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        client.change("incorrect", CHANGED).await.0,
        StatusCode::BAD_REQUEST
    );
    let (status, session) = client.change(INITIAL, CHANGED).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["user"]["must_change_password"], false);
    assert_ne!(old_cookie, client.cookie);
    let mut old = Client {
        router: client.router.clone(),
        cookie: old_cookie,
        csrf: old_csrf,
    };
    assert_eq!(
        old.request("GET", "/api/auth/me", json!(null)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client.request("GET", "/api/users", json!(null)).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn admins_can_publish_but_cannot_manage_any_accounts() {
    let (_dir, mut owner) = setup().await;
    owner_ready(&mut owner).await;
    assert_eq!(
        owner
            .request(
                "POST",
                "/api/users",
                json!({"username":"maria","role":"admin","password":INITIAL})
            )
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        owner
            .request(
                "POST",
                "/api/users",
                json!({"username":"MARIA","role":"owner","password":INITIAL})
            )
            .await
            .0,
        StatusCode::CONFLICT
    );
    let mut admin = Client {
        router: owner.router.clone(),
        cookie: String::new(),
        csrf: String::new(),
    };
    admin.login("maria", INITIAL).await;
    admin.change(INITIAL, CHANGED).await;
    for (method, path, body) in [
        ("GET", "/api/users", json!(null)),
        (
            "POST",
            "/api/users",
            json!({"username":"rogue","role":"owner","password":INITIAL}),
        ),
        ("POST", "/api/users/1/password", json!({"password":INITIAL})),
    ] {
        assert_eq!(
            admin.request(method, path, body).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let (_, mut content) = admin.request("GET", "/api/content", json!(null)).await;
    content["menu_items"][0]["price"] = json!("$12.50");
    content["menu_items"].as_array_mut().unwrap().remove(1);
    content["gallery"].as_array_mut().unwrap().remove(0);
    let (status, published) = admin.request("PUT", "/api/content", content.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(published["revision"], 2);
    assert_eq!(
        admin.request("PUT", "/api/content", content).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        owner.request("GET", "/api/content", json!(null)).await.1["menu_items"][0]["price"],
        "$12.50"
    );
}

#[tokio::test]
async fn owner_can_reset_owners_admins_and_self_invalidating_every_session() {
    let (_dir, mut owner) = setup().await;
    owner_ready(&mut owner).await;
    for role in ["owner", "admin"] {
        owner
            .request(
                "POST",
                "/api/users",
                json!({"username":role,"role":role,"password":INITIAL}),
            )
            .await;
        let mut user = Client {
            router: owner.router.clone(),
            cookie: String::new(),
            csrf: String::new(),
        };
        user.login(role, INITIAL).await;
        user.change(INITIAL, CHANGED).await;
        let (_, users) = owner.request("GET", "/api/users", json!(null)).await;
        let id = users["users"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["username"] == role)
            .unwrap()["id"]
            .as_i64()
            .unwrap();
        let path = format!("/api/users/{id}/password");
        assert_eq!(
            owner
                .request("POST", &path, json!({"password":INITIAL}))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            user.request("GET", "/api/auth/me", json!(null)).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(user.login(role, CHANGED).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(
            user.login(role, INITIAL).await.1["user"]["must_change_password"],
            true
        );
        assert_eq!(
            user.request(
                "POST",
                "/api/users",
                json!({"username":"blocked","role":"admin","password":INITIAL})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        owner
            .request("POST", "/api/users/1/password", json!({"password":INITIAL}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        owner.request("GET", "/api/auth/me", json!(null)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        owner
            .login("jesse.stone.truecore.services", INITIAL)
            .await
            .1["user"]["must_change_password"],
        true
    );
}

#[tokio::test]
async fn csrf_origin_and_content_validation_are_enforced() {
    let (_dir, mut client) = setup().await;
    owner_ready(&mut client).await;
    let (_, content) = client.request("GET", "/api/content", json!(null)).await;
    let token = std::mem::take(&mut client.csrf);
    assert_eq!(
        client
            .request("PUT", "/api/content", content.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    client.csrf = token;
    let response = client
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("origin", "https://evil.example")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = client
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/content")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["access-control-allow-origin"], "*");
    for field in ["id", "category", "price"] {
        let mut invalid = content.clone();
        invalid["menu_items"][0][field] = json!("");
        assert_eq!(
            client.request("PUT", "/api/content", invalid).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let mut invalid = content.clone();
    invalid["gallery"][0]["url"] = json!("javascript:alert(1)");
    assert_eq!(
        client.request("PUT", "/api/content", invalid).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut invalid = content.clone();
    invalid["menu_items"][1]["id"] = invalid["menu_items"][0]["id"].clone();
    assert_eq!(
        client.request("PUT", "/api/content", invalid).await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn login_throttles_and_database_survives_restarts_without_reapplying_bootstrap() {
    let (dir, mut client) = setup().await;
    owner_ready(&mut client).await;
    let mut restarted = config(&dir);
    restarted.bootstrap_password = None;
    let mut client = Client {
        router: app(initialize(restarted).await.unwrap()),
        cookie: String::new(),
        csrf: String::new(),
    };
    assert_eq!(
        client
            .login("jesse.stone.truecore.services", CHANGED)
            .await
            .1["user"]["must_change_password"],
        false
    );
    for _ in 0..8 {
        assert_eq!(
            client.login("nonexistent", "wrong").await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        client.login("nonexistent", "wrong").await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn uploads_require_setup_and_real_images_and_are_served_as_webp() {
    let (_dir, mut client) = setup().await;
    client.login("jesse.stone.truecore.services", INITIAL).await;
    let upload = |client: &Client, bytes: Vec<u8>| {
        let mut body=b"--test-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
        body.extend(bytes);
        body.extend(b"\r\n--test-boundary--\r\n");
        Request::builder()
            .method("POST")
            .uri("/api/uploads")
            .header("origin", ORIGIN)
            .header("cookie", &client.cookie)
            .header("x-csrf-token", &client.csrf)
            .header(
                "content-type",
                "multipart/form-data; boundary=test-boundary",
            )
            .body(Body::from(body))
            .unwrap()
    };
    assert_eq!(
        client
            .router
            .clone()
            .oneshot(upload(&client, b"not an image".to_vec()))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    client.change(INITIAL, CHANGED).await;
    assert_eq!(
        client
            .router
            .clone()
            .oneshot(upload(&client, b"not an image".to_vec()))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(20, 20)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let response = client
        .router
        .clone()
        .oneshot(upload(&client, png.into_inner()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let result: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let url = result["url"].as_str().unwrap();
    assert!(url.ends_with(".webp"));
    let response = client
        .router
        .clone()
        .oneshot(Request::builder().uri(url).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/webp");
    let (_, mut content) = client.request("GET", "/api/content", json!(null)).await;
    content["gallery"][0]["url"] = json!(url);
    assert_eq!(
        client.request("PUT", "/api/content", content).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn dashboard_entrypoint_cookies_and_expired_sessions_work() {
    let (dir, mut client) = setup().await;
    std::fs::create_dir_all(dir.path().join("dist/dashboard")).unwrap();
    std::fs::write(
        dir.path().join("dist/index.html"),
        "<!doctype html><title>Restaurant website</title>",
    )
    .unwrap();
    let dashboard_html = "<!doctype html><title>Business dashboard</title>";
    std::fs::write(dir.path().join("dist/dashboard/index.html"), dashboard_html).unwrap();
    std::fs::write(
        dir.path().join("dist/dashboard/dashboard.js"),
        "console.log('dashboard');",
    )
    .unwrap();
    let response = client
        .router
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("location").is_none());
    assert_eq!(response.headers()["content-type"], "text/html");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        dashboard_html
    );
    for path in ["/dashboard", "/dashboard/"] {
        let response = client
            .router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(response.headers()["location"], "/");
    }
    let response = client
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/dashboard/dashboard.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "console.log('dashboard');"
    );
    client.login("jesse.stone.truecore.services", INITIAL).await;
    let db = rusqlite::Connection::open(dir.path().join("cocina.sqlite3")).unwrap();
    db.execute("UPDATE sessions SET expires_at=0", []).unwrap();
    assert_eq!(
        client.request("GET", "/api/auth/me", json!(null)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let mut secure = config(&dir);
    secure.secure_cookies = true;
    let router = app(initialize(secure).await.unwrap());
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("origin", ORIGIN)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"username":"jesse.stone.truecore.services","password":INITIAL})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(
        cookie.contains("Secure")
            && cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Strict")
    );
}

#[tokio::test]
async fn owners_can_rename_accounts_preserving_sessions_passwords_and_identity() {
    let (dir, mut owner) = setup().await;
    owner_ready(&mut owner).await;
    for role in ["owner", "admin"] {
        assert_eq!(
            owner
                .request(
                    "POST",
                    "/api/users",
                    json!({"username":role,"role":role,"password":INITIAL,"email":format!("{role}@example.com")}),
                )
                .await
                .0,
            StatusCode::CREATED
        );
    }
    let (_, accounts) = owner.request("GET", "/api/users", json!(null)).await;
    let original_cookie = owner.cookie.clone();
    let db = rusqlite::Connection::open(dir.path().join("cocina.sqlite3")).unwrap();
    for account in accounts["users"].as_array().unwrap() {
        let id = account["id"].as_i64().unwrap();
        let old_name = account["username"].as_str().unwrap();
        let password = if id == 1 { CHANGED } else { INITIAL };
        let mut existing = Client {
            router: owner.router.clone(),
            cookie: String::new(),
            csrf: String::new(),
        };
        assert_eq!(existing.login(old_name, password).await.0, StatusCode::OK);
        for name in [old_name.to_string(), format!("renamed-{id}")] {
            db.execute(
                "INSERT INTO login_attempts(key,attempts,window_start) VALUES(?1,8,strftime('%s','now'))",
                [format!("user:{name}")],
            )
            .unwrap();
        }
        let global_before: i64 = db
            .query_row(
                "SELECT attempts FROM login_attempts WHERE key='global'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let path = format!("/api/users/{id}/username");
        let (status, result) = owner
            .request(
                "PUT",
                &path,
                json!({"username":format!("  RENAMED-{id}  ")}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["user"]["username"], format!("renamed-{id}"));
        for field in ["id", "role", "email", "must_change_password"] {
            assert_eq!(result["user"][field], account[field]);
        }
        assert!(!result.to_string().contains("password_hash"));
        assert_eq!(owner.cookie, original_cookie);
        let attempts: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM login_attempts WHERE key IN (?1,?2)",
                [format!("user:{old_name}"), format!("user:renamed-{id}")],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(attempts, 0);
        assert_eq!(
            db.query_row(
                "SELECT attempts FROM login_attempts WHERE key='global'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            global_before
        );
        let (status, session) = existing.request("GET", "/api/auth/me", json!(null)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(session["user"], result["user"]);
        assert_eq!(
            existing.login(old_name, password).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            existing.login(&format!("renamed-{id}"), password).await.0,
            StatusCode::OK
        );
        // Normalized retries of the same name are allowed without another rename.
        assert_eq!(
            owner
                .request("PUT", &path, json!({"username":format!("RENAMED-{id}")}))
                .await
                .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        owner.request("GET", "/api/users", json!(null)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        owner.request("GET", "/api/auth/me", json!(null)).await.1["user"]["username"],
        "renamed-1"
    );
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM audit WHERE action='username_updated' AND actor_id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 3);
    // Startup retains the renamed owner instead of recreating the bootstrap name.
    let restarted = app(initialize(config(&dir)).await.unwrap());
    let mut after_restart = Client {
        router: restarted,
        cookie: original_cookie,
        csrf: owner.csrf.clone(),
    };
    let (status, session) = after_restart
        .request("GET", "/api/auth/me", json!(null))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["user"]["username"], "renamed-1");
    assert_eq!(
        after_restart
            .request("GET", "/api/users", json!(null))
            .await
            .1["users"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn username_changes_enforce_owner_access_validation_and_uniqueness() {
    let (_dir, mut owner) = setup().await;
    let path = "/api/users/1/username";
    let body = json!({"username":"new.owner"});
    assert_eq!(
        owner.request("PUT", path, body.clone()).await.0,
        StatusCode::UNAUTHORIZED
    );
    owner.login("jesse.stone.truecore.services", INITIAL).await;
    assert_eq!(
        owner.request("PUT", path, body.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    owner.change(INITIAL, CHANGED).await;
    assert_eq!(
        owner
            .request(
                "POST",
                "/api/users",
                json!({"username":"admin", "role":"admin", "password":INITIAL})
            )
            .await
            .0,
        StatusCode::CREATED
    );
    let mut admin = Client {
        router: owner.router.clone(),
        cookie: String::new(),
        csrf: String::new(),
    };
    admin.login("admin", INITIAL).await;
    admin.change(INITIAL, CHANGED).await;
    for id in [1, 2] {
        assert_eq!(
            admin
                .request("PUT", &format!("/api/users/{id}/username"), body.clone())
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let csrf = std::mem::take(&mut owner.csrf);
    assert_eq!(
        owner.request("PUT", path, body.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    owner.csrf = csrf;
    let response = owner
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(path)
                .header("origin", "https://other.example.com")
                .header("cookie", &owner.cookie)
                .header("x-csrf-token", &owner.csrf)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    for invalid in [
        "",
        "ab",
        "with spaces",
        "a/b",
        "<script>",
        "mariá",
        &"a".repeat(101),
    ] {
        assert_eq!(
            owner
                .request("PUT", path, json!({"username":invalid}))
                .await
                .0,
            StatusCode::BAD_REQUEST,
            "{invalid:?}"
        );
    }
    let (status, error) = owner
        .request("PUT", path, json!({"username":"  ADMIN  "}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["code"], "username_taken");
    assert_eq!(
        owner
            .request("PUT", "/api/users/99999/username", body)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        owner.request("GET", "/api/auth/me", json!(null)).await.1["user"]["username"],
        "jesse.stone.truecore.services"
    );
    assert_eq!(
        admin.request("GET", "/api/auth/me", json!(null)).await.1["user"]["username"],
        "admin"
    );
    // A boundary-length name with every permitted punctuation mark remains valid.
    let valid = format!("{}._@+-", "a".repeat(95));
    let (status, result) = owner.request("PUT", path, json!({"username":valid})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["user"]["username"], valid);
}

#[tokio::test]
async fn owners_can_edit_contact_emails_for_themselves_owners_and_admins() {
    let (dir, mut owner) = setup().await;
    owner_ready(&mut owner).await;
    let original_cookie = owner.cookie.clone();
    for role in ["owner", "admin"] {
        assert_eq!(
            owner
                .request(
                    "POST",
                    "/api/users",
                    json!({
                        "username": role, "role": role, "password": INITIAL,
                        "email": format!("{role}@example.com")
                    })
                )
                .await
                .0,
            StatusCode::CREATED
        );
    }
    let (_, accounts) = owner.request("GET", "/api/users", json!(null)).await;
    for account in accounts["users"].as_array().unwrap() {
        let id = account["id"].as_i64().unwrap();
        let path = format!("/api/users/{id}/email");
        let email = format!("updated-{id}@example.com");
        let (status, result) = owner
            .request(
                "PUT",
                &path,
                json!({"email": format!("  UPDATED-{id}@Example.com  ")}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["user"]["email"], email);
        assert_eq!(result["user"]["username"], account["username"]);
        assert_eq!(result["user"]["role"], account["role"]);
        assert_eq!(
            result["user"]["must_change_password"],
            account["must_change_password"]
        );
        // Re-saving an account's own address does not conflict with itself.
        assert_eq!(
            owner.request("PUT", &path, json!({"email": email})).await.0,
            StatusCode::OK
        );
    }
    assert_eq!(owner.cookie, original_cookie);
    let (_, session) = owner.request("GET", "/api/auth/me", json!(null)).await;
    assert_eq!(session["user"]["email"], "updated-1@example.com");
    let (_, session) = owner.login("jesse.stone.truecore.services", CHANGED).await;
    assert_eq!(session["user"]["email"], "updated-1@example.com");
    let (status, result) = owner
        .request("PUT", "/api/users/1/email", json!({"email": "   "}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(result["user"]["email"].is_null());
    let db = rusqlite::Connection::open(dir.path().join("cocina.sqlite3")).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM audit WHERE action='email_updated' AND actor_id=1 AND target='1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn email_changes_enforce_owner_access_csrf_validation_and_uniqueness() {
    let (_dir, mut owner) = setup().await;
    let path = "/api/users/1/email";
    let body = json!({"email": "owner@example.com"});
    assert_eq!(
        owner.request("PUT", path, body.clone()).await.0,
        StatusCode::UNAUTHORIZED
    );
    owner.login("jesse.stone.truecore.services", INITIAL).await;
    assert_eq!(
        owner.request("PUT", path, body.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(owner.change(INITIAL, CHANGED).await.0, StatusCode::OK);
    assert_eq!(owner.request("POST", "/api/users", json!({"username":"admin", "role":"admin", "password":INITIAL, "email":"admin@example.com"})).await.0, StatusCode::CREATED);
    let mut admin = Client {
        router: owner.router.clone(),
        cookie: String::new(),
        csrf: String::new(),
    };
    admin.login("admin", INITIAL).await;
    admin.change(INITIAL, CHANGED).await;
    for id in [1, 2] {
        assert_eq!(
            admin
                .request("PUT", &format!("/api/users/{id}/email"), body.clone())
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let csrf = std::mem::take(&mut owner.csrf);
    assert_eq!(
        owner.request("PUT", path, body.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    owner.csrf = csrf;
    let request = Request::builder()
        .method("PUT")
        .uri(path)
        .header("origin", "https://other.example.com")
        .header("cookie", &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    assert_eq!(
        owner
            .router
            .clone()
            .oneshot(request)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for invalid in [
        "invalid",
        "no@domain",
        "a..b@example.com",
        "a@@example.com",
        "@example.com",
        "x@-example.com",
        "x@a..com",
        "x@exa mple.com",
        "x\r\n@example.com",
    ] {
        assert_eq!(
            owner.request("PUT", path, json!({"email":invalid})).await.0,
            StatusCode::BAD_REQUEST,
            "{invalid:?}"
        );
    }
    assert_eq!(
        owner
            .request(
                "PUT",
                path,
                json!({"email":format!("{}@example.com", "x".repeat(65))})
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, error) = owner
        .request("PUT", path, json!({"email":"ADMIN@EXAMPLE.COM"}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["code"], "email_taken");
    assert_eq!(
        owner.request("PUT", "/api/users/99999/email", body).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(owner.request("POST", "/api/users", json!({"username":"duplicate", "role":"owner", "password":INITIAL, "email":"ADMIN@EXAMPLE.COM"})).await.0, StatusCode::CONFLICT);
    assert_eq!(owner.request("POST", "/api/users", json!({"username":"invalid", "role":"owner", "password":INITIAL, "email":"bad-email"})).await.0, StatusCode::BAD_REQUEST);
    // An admin's existing session sees its changed email without being revoked.
    assert_eq!(
        owner
            .request(
                "PUT",
                "/api/users/2/email",
                json!({"email":"maria+team@example.com"})
            )
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        admin.request("GET", "/api/auth/me", json!(null)).await.1["user"]["email"],
        "maria+team@example.com"
    );
}

#[tokio::test]
async fn email_migration_preserves_existing_accounts_content_and_active_sessions() {
    let (dir, mut owner) = setup().await;
    owner_ready(&mut owner).await;
    let (_, original_content) = owner.request("GET", "/api/content", json!(null)).await;
    {
        // Recreate the pre-email users table shape to exercise an actual upgrade.
        let db = rusqlite::Connection::open(dir.path().join("cocina.sqlite3")).unwrap();
        db.execute_batch("DROP INDEX users_email; ALTER TABLE users DROP COLUMN email;")
            .unwrap();
    }
    let mut upgraded = config(&dir);
    upgraded.bootstrap_password = None;
    owner.router = app(initialize(upgraded.clone()).await.unwrap());
    let (status, session) = owner.request("GET", "/api/auth/me", json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["user"]["username"], "jesse.stone.truecore.services");
    assert_eq!(session["user"]["must_change_password"], false);
    assert!(session["user"]["email"].is_null());
    assert_eq!(
        owner.request("GET", "/api/content", json!(null)).await.1,
        original_content
    );
    assert_eq!(
        owner
            .request(
                "PUT",
                "/api/users/1/email",
                json!({"email":"jesse@example.com"})
            )
            .await
            .0,
        StatusCode::OK
    );
    // Repeated startup is safe and keeps the saved email and current session.
    owner.router = app(initialize(upgraded).await.unwrap());
    assert_eq!(
        owner.request("GET", "/api/auth/me", json!(null)).await.1["user"]["email"],
        "jesse@example.com"
    );
    assert_eq!(
        owner
            .login("jesse.stone.truecore.services", CHANGED)
            .await
            .0,
        StatusCode::OK
    );
}
