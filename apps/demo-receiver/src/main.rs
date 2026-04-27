use std::{collections::HashMap, env, net::SocketAddr, sync::Arc};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::info;

type CallbackStore = Arc<RwLock<HashMap<String, Vec<RecordedCallback>>>>;

#[derive(Clone)]
struct AppState {
    store: CallbackStore,
}

#[derive(Debug, Clone, Serialize)]
struct RecordedCallback {
    key: String,
    behavior: String,
    attempt_no: usize,
    received_at: DateTime<Utc>,
    payload: Value,
    signature: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct HealthResponse {
    status: String,
}

#[derive(Debug, Clone, Serialize)]
struct CallbackAttemptsResponse {
    key: String,
    attempts: Vec<RecordedCallback>,
}

#[derive(Debug, Clone, Serialize)]
struct CallbackKeysResponse {
    keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CallbackPostResponse {
    key: String,
    behavior: String,
    attempt_no: usize,
    accepted: bool,
    note: String,
}

#[derive(Debug, Clone, Serialize)]
struct ResetResponse {
    cleared_keys: usize,
    cleared_attempts: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiverBehavior {
    AlwaysSuccess,
    FailOnce,
    FailTwice,
    AlwaysFail,
}

impl ReceiverBehavior {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "always_success" => Some(Self::AlwaysSuccess),
            "fail_once" => Some(Self::FailOnce),
            "fail_twice" => Some(Self::FailTwice),
            "always_fail" => Some(Self::AlwaysFail),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::AlwaysSuccess => "always_success",
            Self::FailOnce => "fail_once",
            Self::FailTwice => "fail_twice",
            Self::AlwaysFail => "always_fail",
        }
    }

    fn should_accept(self, attempt_no: usize) -> bool {
        match self {
            Self::AlwaysSuccess => true,
            Self::FailOnce => attempt_no > 1,
            Self::FailTwice => attempt_no > 2,
            Self::AlwaysFail => false,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let bind_addr =
        env::var("DEMO_RECEIVER_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3020".to_string());

    let state = AppState {
        store: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/callbacks", get(list_callback_keys))
        .route("/callbacks/{key}", get(get_callback_attempts))
        .route("/callbacks/{behavior}/{key}", post(receive_callback))
        .route("/admin/reset", post(reset))
        .with_state(state);

    let addr: SocketAddr = bind_addr.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("demo receiver listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

async fn list_callback_keys(State(state): State<AppState>) -> Json<CallbackKeysResponse> {
    let store = state.store.read().await;
    let mut keys = store.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    Json(CallbackKeysResponse { keys })
}

async fn get_callback_attempts(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<CallbackAttemptsResponse>, (StatusCode, Json<Value>)> {
    let store = state.store.read().await;
    let attempts = store.get(&key).cloned().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("callback key not found: {key}")
            })),
        )
    })?;

    Ok(Json(CallbackAttemptsResponse { key, attempts }))
}

async fn receive_callback(
    State(state): State<AppState>,
    Path((behavior_raw, key)): Path<(String, String)>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<(StatusCode, Json<CallbackPostResponse>), (StatusCode, Json<Value>)> {
    let behavior = ReceiverBehavior::parse(&behavior_raw).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("unsupported callback behavior: {behavior_raw}")
            })),
        )
    })?;

    let signature = headers
        .get("X-Gateway-Signature")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());

    let (attempt_no, accepted) = {
        let mut store = state.store.write().await;
        let attempts = store.entry(key.clone()).or_default();
        let attempt_no = attempts.len() + 1;
        let accepted = behavior.should_accept(attempt_no);

        attempts.push(RecordedCallback {
            key: key.clone(),
            behavior: behavior.as_str().to_string(),
            attempt_no,
            received_at: Utc::now(),
            payload,
            signature,
        });

        (attempt_no, accepted)
    };

    let response = CallbackPostResponse {
        key,
        behavior: behavior.as_str().to_string(),
        attempt_no,
        accepted,
        note: if accepted {
            "demo receiver accepted callback".to_string()
        } else {
            "demo receiver rejected callback on purpose".to_string()
        },
    };

    let status = if accepted {
        StatusCode::OK
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };

    Ok((status, Json(response)))
}

async fn reset(State(state): State<AppState>) -> Json<ResetResponse> {
    let (cleared_keys, cleared_attempts) = {
        let mut store = state.store.write().await;
        let cleared_keys = store.len();
        let cleared_attempts = store.values().map(Vec::len).sum();
        store.clear();
        (cleared_keys, cleared_attempts)
    };

    Json(ResetResponse {
        cleared_keys,
        cleared_attempts,
    })
}

#[cfg(test)]
mod tests {
    use super::ReceiverBehavior;

    #[test]
    fn fail_once_accepts_on_second_attempt() {
        assert!(!ReceiverBehavior::FailOnce.should_accept(1));
        assert!(ReceiverBehavior::FailOnce.should_accept(2));
    }

    #[test]
    fn fail_twice_accepts_on_third_attempt() {
        assert!(!ReceiverBehavior::FailTwice.should_accept(1));
        assert!(!ReceiverBehavior::FailTwice.should_accept(2));
        assert!(ReceiverBehavior::FailTwice.should_accept(3));
    }
}
