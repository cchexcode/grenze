use axum::{extract::State, http::{header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE}, HeaderMap, Method, StatusCode}, response::IntoResponse, Json};
use anyhow::Result;
use redis::Script;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{sync::Arc, time::{SystemTime, UNIX_EPOCH, Duration}};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub http_client: reqwest::Client,
    pub redis: Arc<Mutex<redis::aio::MultiplexedConnection>>,
    pub capacity: u32,
    pub leak_per_sec: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProxyRequest {
    // Mandatory rate limit key supplied by the client
    pub key: String,
    pub url: String,
    pub method: String,
    pub headers: std::collections::HashMap<String, String>,
    pub query: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

pub async fn proxy(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Json(req): axum::extract::Json<ProxyRequest>,
) -> impl IntoResponse {
    // Require and enforce caller-provided rate limit key
    let key = req.key.trim().to_string();
    if key.is_empty() {
        let payload = Json(json!({
            "error": "missing_key",
            "message": "Request must include non-empty 'key'"
        }));
        return (StatusCode::BAD_REQUEST, payload).into_response();
    }
    if !state.allow(&key).await {
        let payload = Json(json!({
            "error": "rate_limited",
            "message": "Too many requests"
        }));
        return (StatusCode::TOO_MANY_REQUESTS, payload).into_response();
    }

    // Validate URL and method (consider allowlists in production)
    let dest = req.url;
    let method = req.method.to_uppercase();
    let parsed_method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::POST);

    // Build downstream request
    let mut builder = state.http_client.request(parsed_method, &dest);

    // Add query params
    if !req.query.is_empty() {
        builder = builder.query(&req.query);
    }

    // Add headers from JSON (string pairs)
    for (k, v) in req.headers {
        builder = builder.header(k, v);
    }

    // Pass through Accept if provided by caller as a header
    if let Some(acc) = headers.get("accept").and_then(|h| h.to_str().ok()) {
        builder = builder.header("accept", acc);
    }

    // Timeout
    if let Some(ms) = req.timeout_ms {
        builder = builder.timeout(std::time::Duration::from_millis(ms));
    }

    // Body
    let downstream = match match req.body {
        Some(b) => builder.json(&b).send().await,
        None => builder.send().await,
    } {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":"downstream_error","message": e.to_string()})),
            )
                .into_response();
        }
    };

    let status = StatusCode::from_u16(downstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut resp_headers = HeaderMap::new();
    for (name, value) in downstream.headers().iter() {
        // pass through limited safe headers
        if name == CONTENT_TYPE || name == CONTENT_LENGTH || name == CACHE_CONTROL {
            resp_headers.insert(name.clone(), value.clone());
        }
    }
    let bytes = match downstream.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":"downstream_read_error","message": e.to_string()})),
            )
                .into_response();
        }
    };

    (status, resp_headers, bytes).into_response()
}

impl AppState {
    pub async fn new(rps: u32, redis_url: &str) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .user_agent("grenze-server-proxy/0.0.0")
            .build()
            .expect("failed to build reqwest client");

        let client = redis::Client::open(redis_url).expect("invalid redis url");
        let conn = {
            let mut attempt: u32 = 0;
            loop {
                attempt += 1;
                match client.get_multiplexed_tokio_connection().await {
                    Ok(c) => break c,
                    Err(_e) if attempt < 30 => {
                        tokio::time::sleep(Duration::from_millis(200 * attempt as u64)).await;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };

        Ok(Self {
            http_client,
            redis: Arc::new(Mutex::new(conn)),
            capacity: rps,
            leak_per_sec: rps as f64,
        })
    }

    pub async fn allow(&self, key: &str) -> bool {
        let bucket_key = format!("rl:{}", key);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let ttl_secs: i64 = ((self.capacity as f64) / self.leak_per_sec).ceil() as i64 + 1;

        // Redis Lua script implementing a leaky bucket
        // Returns 1 if allowed and increments the bucket, 0 otherwise
        const LUA: &str = r#"
local base = KEYS[1]
local fill_key = base .. ":fill"
local ts_key = base .. ":ts"

local capacity = tonumber(ARGV[1])
local leak_per_sec = tonumber(ARGV[2])
local now_ms = tonumber(ARGV[3])
local ttl = tonumber(ARGV[4])

local fill = tonumber(redis.call('GET', fill_key) or '0')
local last = tonumber(redis.call('GET', ts_key) or now_ms)
local elapsed_ms = now_ms - last
if elapsed_ms < 0 then elapsed_ms = 0 end

local leaked = (elapsed_ms / 1000.0) * leak_per_sec
fill = fill - leaked
if fill < 0 then fill = 0 end

if (fill + 1) > capacity then
  -- Update timestamp to avoid burst after long idle and set TTLs
  redis.call('SET', ts_key, now_ms)
  redis.call('EXPIRE', ts_key, ttl)
  redis.call('SET', fill_key, tostring(fill))
  redis.call('EXPIRE', fill_key, ttl)
  return 0
end

fill = fill + 1
redis.call('SET', fill_key, tostring(fill))
redis.call('EXPIRE', fill_key, ttl)
redis.call('SET', ts_key, now_ms)
redis.call('EXPIRE', ts_key, ttl)
return 1
"#;

        let script = Script::new(LUA);
        let mut conn = self.redis.lock().await;
        match script
            .key(bucket_key)
            .arg(self.capacity as i64)
            .arg(self.leak_per_sec)
            .arg(now_ms)
            .arg(ttl_secs)
            .invoke_async::<i64>(&mut *conn)
            .await
        {
            Ok(1) => true,
            Ok(_) => false,
            Err(_) => false,
        }
    }
}
