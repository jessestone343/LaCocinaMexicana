use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAPS_URL: &str = "https://share.google/jdQG4F1zb468JzTCB";
const FIELDS: &str = "id,displayName,rating,userRatingCount,googleMapsUri,reviews";

pub(super) struct Reviews {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    api_key: String,
    key_name: Option<&'static str>,
    place_id: String,
    limit: usize,
    cache: Mutex<Option<Cached>>,
}

struct Cached {
    expires: Instant,
    body: Value,
}

#[derive(Deserialize)]
pub(super) struct ReviewsQuery {
    debug: Option<String>,
}

pub(super) async fn handler(
    State(state): State<crate::Shared>,
    Query(query): Query<ReviewsQuery>,
) -> Response {
    let body = state.reviews.get().await;
    let status = if query.debug.as_deref() == Some("1")
        && body["reviews"].as_array().is_none_or(Vec::is_empty)
    {
        if body["configured"] == true {
            StatusCode::BAD_GATEWAY
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        }
    } else {
        StatusCode::OK
    };
    // The shared security middleware sets no-store on public API responses.
    (status, Json(body)).into_response()
}

fn review_limit(value: &str) -> usize {
    value
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0)
        .unwrap_or(3)
        .clamp(1, 5)
}

impl Reviews {
    pub(super) fn from_env() -> Result<Self, reqwest::Error> {
        let (key, name) = ["GOOGLE_PLACES_API_KEY", "GOOGLE_MAPS_API_KEY"]
            .into_iter()
            .find_map(|name| {
                std::env::var(name)
                    .ok()
                    .filter(|v| !v.is_empty())
                    .map(|v| (v, Some(name)))
            })
            .unwrap_or_default();
        Self::new(
            key,
            name,
            std::env::var("GOOGLE_PLACE_ID").unwrap_or_default(),
            review_limit(&std::env::var("GOOGLE_REVIEWS_LIMIT").unwrap_or_default()),
        )
    }

    fn new(
        api_key: String,
        key_name: Option<&'static str>,
        place_id: String,
        limit: usize,
    ) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            endpoint: reqwest::Url::parse("https://places.googleapis.com/v1/places/").unwrap(),
            api_key,
            key_name,
            place_id,
            limit,
            cache: Mutex::new(None),
        })
    }

    async fn get(&self) -> Value {
        // Hold only this endpoint's async mutex during a refresh, so simultaneous
        // visitors share a single Google request without affecting dashboard work.
        let mut cache = self.cache.lock().await;
        if let Some(cached) = cache.as_ref().filter(|v| v.expires > Instant::now()) {
            return cached.body.clone();
        }
        let body = self.fetch().await;
        let seconds = if body["reviews"].as_array().is_some_and(|r| !r.is_empty()) {
            21600
        } else {
            60
        };
        *cache = Some(Cached {
            expires: Instant::now() + Duration::from_secs(seconds),
            body: body.clone(),
        });
        body
    }

    async fn fetch(&self) -> Value {
        let configured = !self.api_key.is_empty() && !self.place_id.is_empty();
        let mut body = json!({
            "source": "Google Business Profile", "configured": configured,
            "place": {"name":"La Cocina Mexicana", "googleMapsUri":MAPS_URL},
            "reviews": [],
            "diagnostics": {"env": {
                "apiKeyPresent":!self.api_key.is_empty(), "apiKeyEnvName":self.key_name,
                "placeIdPresent":!self.place_id.is_empty(), "placeIdLength":self.place_id.len(), "reviewLimit":self.limit
            }, "attempts":[]}
        });
        if !configured {
            body["message"] = json!("Google Places API environment variables are missing.");
            body["requiredEnv"] = json!(["GOOGLE_PLACES_API_KEY", "GOOGLE_PLACE_ID"]);
            return body;
        }
        let mut url = self.endpoint.clone();
        url.path_segments_mut()
            .unwrap()
            .pop_if_empty()
            .push(&self.place_id);
        url.query_pairs_mut()
            .append_pair("languageCode", "en")
            .append_pair("regionCode", "US");
        let result = self
            .client
            .get(url)
            .header("X-Goog-Api-Key", &self.api_key)
            .header("X-Goog-FieldMask", FIELDS)
            .send()
            .await;
        let (http_status, payload) = match result {
            Ok(response) => (
                Some(response.status().as_u16()),
                response.json::<Value>().await.ok(),
            ),
            Err(_) => (None, None),
        };
        let ok = http_status.is_some_and(|s| (200..300).contains(&s)) && payload.is_some();
        let mut attempt = json!({"provider":"places-api-new", "ok":ok, "httpStatus":http_status, "googleStatus":null, "error":null});
        if !ok {
            let message = payload
                .as_ref()
                .and_then(|p| p.pointer("/error/message"))
                .and_then(Value::as_str)
                .unwrap_or("Unable to reach Google Places. Please try again later.");
            // Never expose the credential, even if an upstream error echoes it.
            let message: String = message
                .replace(&self.api_key, "[redacted]")
                .chars()
                .take(1000)
                .collect();
            attempt["error"] = json!(message);
            body["unavailable"] = json!(true);
            body["message"] = json!(format!(
                "Google reviews are temporarily unavailable: {message}"
            ));
        } else {
            let place = payload.unwrap();
            let raw = place["reviews"].as_array().cloned().unwrap_or_default();
            let normalized: Vec<Value> = raw.iter().map(|r| json!({
                "author":r.pointer("/authorAttribution/displayName").and_then(Value::as_str).unwrap_or("Google reviewer"),
                "authorUri":r.pointer("/authorAttribution/uri").and_then(Value::as_str).unwrap_or(""),
                "rating":r["rating"].as_f64().unwrap_or(0.0),
                "text":r.pointer("/text/text").and_then(Value::as_str).unwrap_or(""),
                "relativeTime":r["relativePublishTimeDescription"].as_str().unwrap_or(""),
                "publishedAt":r["publishTime"].as_str().unwrap_or(""),
                "googleMapsUri":r["googleMapsUri"].as_str().unwrap_or("")
            })).collect();
            let eligible = |r: &&Value| {
                r["rating"].as_f64() == Some(5.0)
                    && r["text"].as_str().is_some_and(|t| !t.is_empty())
            };
            let reviews: Vec<Value> = normalized
                .iter()
                .filter(eligible)
                .take(self.limit)
                .cloned()
                .collect();
            attempt["rawReviewCount"] = json!(raw.len());
            attempt["textReviewCount"] = json!(
                normalized
                    .iter()
                    .filter(|r| r["text"].as_str().is_some_and(|t| !t.is_empty()))
                    .count()
            );
            attempt["fiveStarReviewCount"] = json!(
                normalized
                    .iter()
                    .filter(|r| r["rating"].as_f64() == Some(5.0))
                    .count()
            );
            attempt["fiveStarTextReviewCount"] = json!(normalized.iter().filter(eligible).count());
            attempt["ratingsReturned"] =
                json!(normalized.iter().map(|r| &r["rating"]).collect::<Vec<_>>());
            attempt["returnedReviews"] = json!(normalized.iter().map(|r| json!({
                "author":r["author"], "rating":r["rating"], "hasText":!r["text"].as_str().unwrap_or("").is_empty(),
                "textLength":r["text"].as_str().unwrap_or("").chars().count(), "relativeTime":r["relativeTime"]
            })).collect::<Vec<_>>());
            body["provider"] = json!("places-api-new");
            body["place"] = json!({
                "id":place["id"].as_str().unwrap_or(&self.place_id),
                "name":place.pointer("/displayName/text").and_then(Value::as_str).unwrap_or("La Cocina Mexicana"),
                "rating":place["rating"], "userRatingCount":place["userRatingCount"],
                "googleMapsUri":place["googleMapsUri"].as_str().unwrap_or(MAPS_URL)
            });
            if reviews.is_empty() {
                body["unavailable"] = json!(true);
                body["message"] = json!(
                    "Google did not return any written 5-star reviews in its Places response."
                );
            }
            body["reviews"] = json!(reviews);
        }
        body["diagnostics"]["attempts"] = json!([attempt]);
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::HeaderMap, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    async fn fixture(
        payload: Value,
        status: StatusCode,
    ) -> (Reviews, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let router = Router::new().route(
            "/v1/places/{id}",
            get(move |headers: HeaderMap| {
                let count = count.clone();
                let payload = payload.clone();
                async move {
                    assert_eq!(headers["x-goog-api-key"], "fake-key");
                    assert_eq!(headers["x-goog-fieldmask"], FIELDS);
                    count.fetch_add(1, Ordering::SeqCst);
                    (status, Json(payload))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut reviews = Reviews::new(
            "fake-key".into(),
            Some("GOOGLE_PLACES_API_KEY"),
            "place-id".into(),
            1,
        )
        .unwrap();
        reviews.client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        reviews.endpoint = reqwest::Url::parse(&format!("http://{address}/v1/places/")).unwrap();
        (reviews, calls, task)
    }

    #[tokio::test]
    async fn filters_normalizes_and_caches_concurrent_requests() {
        let (reviews, calls, task) = fixture(json!({"displayName":{"text":"La Cocina"},"rating":4.5,"userRatingCount":487,"reviews":[
            {"rating":4,"text":{"text":"Good"}}, {"rating":5},
            {"rating":5,"text":{"text":"Delicious"},"authorAttribution":{"displayName":"Guest","uri":"https://example.com/author"},"publishTime":"2026-01-01T00:00:00Z"},
            {"rating":5,"text":{"text":"Excellent"}}
        ]}), StatusCode::OK).await;
        let (a, b, c) = tokio::join!(reviews.get(), reviews.get(), reviews.get());
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a["reviews"].as_array().unwrap().len(), 1);
        assert_eq!(a["reviews"][0]["text"], "Delicious");
        assert_eq!(a["reviews"][0]["author"], "Guest");
        assert_eq!(a["place"]["userRatingCount"], 487);
        assert_eq!(
            a["diagnostics"]["attempts"][0]["fiveStarTextReviewCount"],
            2
        );
        assert!(!a.to_string().contains("fake-key"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        reviews.cache.lock().await.as_mut().unwrap().expires = Instant::now();
        reviews.get().await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        task.abort();
    }

    #[tokio::test]
    async fn upstream_errors_are_redacted_and_briefly_cached() {
        let (reviews, calls, task) = fixture(
            json!({"error":{"message":"Rejected fake-key"}}),
            StatusCode::FORBIDDEN,
        )
        .await;
        let body = reviews.get().await;
        assert_eq!(body["unavailable"], true);
        assert_eq!(body["diagnostics"]["attempts"][0]["httpStatus"], 403);
        assert!(!body.to_string().contains("fake-key"));
        reviews.get().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        task.abort();
        reviews.cache.lock().await.as_mut().unwrap().expires = Instant::now();
        assert_eq!(reviews.get().await["unavailable"], true);
    }

    #[tokio::test]
    async fn missing_configuration_is_reported_without_upstream_requests() {
        let reviews = Reviews::new(String::new(), None, String::new(), 3).unwrap();
        let body = reviews.get().await;
        assert_eq!(body["configured"], false);
        assert_eq!(body["reviews"], json!([]));
        assert_eq!(body["diagnostics"]["env"]["apiKeyPresent"], false);
    }

    #[tokio::test]
    async fn public_route_works_without_a_dashboard_session_and_reports_debug_status() {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let dir = tempfile::TempDir::new().unwrap();
        let mut state = crate::initialize(crate::Config {
            listen: "127.0.0.1:0".into(),
            origin: "http://localhost:5173".into(),
            secure_cookies: false,
            data_dir: dir.path().into(),
            static_dir: dir.path().join("dist"),
            bootstrap_username: "owner".into(),
            bootstrap_password: Some("temporary-test-passphrase".into()),
        })
        .await
        .unwrap();
        Arc::get_mut(&mut state).unwrap().reviews =
            Reviews::new(String::new(), None, String::new(), 3).unwrap();
        let router = crate::app(state);
        for (path, status) in [
            ("/api/google-reviews", StatusCode::OK),
            (
                "/api/google-reviews?debug=1",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["source"], "Google Business Profile");
            assert_eq!(body["configured"], false);
        }
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/google-reviews")
                    .header("Origin", "http://localhost:5173")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn limit_defaults_and_clamps() {
        assert_eq!(review_limit(""), 3);
        assert_eq!(review_limit("0"), 3);
        assert_eq!(review_limit("garbage"), 3);
        assert_eq!(review_limit("1"), 1);
        assert_eq!(review_limit("99"), 5);
    }
}
