use axum::{response::IntoResponse, routing::get, routing::post, Json, Router};
use serde::Deserialize;

#[derive(Deserialize)]
struct ProposeRequest {
    prompt: String,
}

#[derive(Deserialize)]
struct MemoryEditRequest {
    agent: String,
    path: String,
    content: String,
}

pub async fn serve(port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/snapshot", get(snapshot))
        .route("/propose", post(propose))
        .route("/propose-memory-edit", post(propose_memory_edit));
    let address = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&address).await?;
    eprintln!("Brain Steward proposal service listening at http://{address}");
    eprintln!("Approval is intentionally unavailable here; use the HarnessKit UI.");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn snapshot() -> Result<Json<hk_core::brain::BrainSnapshot>, ServiceError> {
    run_blocking(|| hk_core::brain::snapshot(&home_dir()?)).await
}

async fn propose(
    Json(request): Json<ProposeRequest>,
) -> Result<Json<hk_core::steward::StewardProposal>, ServiceError> {
    run_blocking(move || hk_core::steward::propose(&home_dir()?, &request.prompt)).await
}

async fn propose_memory_edit(
    Json(request): Json<MemoryEditRequest>,
) -> Result<Json<hk_core::steward::StewardProposal>, ServiceError> {
    run_blocking(move || {
        hk_core::steward::propose_memory_edit(
            &home_dir()?,
            &request.agent,
            &request.path,
            &request.content,
        )
    })
    .await
}

fn home_dir() -> Result<std::path::PathBuf, hk_core::HkError> {
    dirs::home_dir()
        .ok_or_else(|| hk_core::HkError::Internal("Cannot determine home directory".into()))
}

async fn run_blocking<T: serde::Serialize + Send + 'static>(
    task: impl FnOnce() -> Result<T, hk_core::HkError> + Send + 'static,
) -> Result<Json<T>, ServiceError> {
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|error| ServiceError::internal(error.to_string()))?
        .map(Json)
        .map_err(ServiceError::from)
}

struct ServiceError(axum::http::StatusCode, hk_core::HkError);

impl ServiceError {
    fn internal(message: String) -> Self {
        Self(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            hk_core::HkError::Internal(message),
        )
    }
}

impl From<hk_core::HkError> for ServiceError {
    fn from(error: hk_core::HkError) -> Self {
        let status = match error {
            hk_core::HkError::Validation(_) => axum::http::StatusCode::BAD_REQUEST,
            hk_core::HkError::NotFound(_) => axum::http::StatusCode::NOT_FOUND,
            hk_core::HkError::Conflict(_) => axum::http::StatusCode::CONFLICT,
            _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self(status, error)
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(self.1)).into_response()
    }
}
