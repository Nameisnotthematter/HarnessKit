use axum::Json;
use serde::Deserialize;

use crate::router::{blocking, ApiError};

type Result<T> = std::result::Result<Json<T>, ApiError>;

fn home_dir() -> std::result::Result<std::path::PathBuf, hk_core::HkError> {
    dirs::home_dir()
        .ok_or_else(|| hk_core::HkError::Internal("Cannot determine home directory".into()))
}

pub async fn brain_snapshot() -> Result<hk_core::brain::BrainSnapshot> {
    blocking(move || hk_core::brain::snapshot(&home_dir()?)).await
}

#[derive(Deserialize)]
pub struct ProposeParams {
    pub prompt: String,
}

pub async fn steward_propose(
    Json(params): Json<ProposeParams>,
) -> Result<hk_core::steward::StewardProposal> {
    blocking(move || hk_core::steward::propose(&home_dir()?, &params.prompt)).await
}

#[derive(Deserialize)]
pub struct ApproveParams {
    pub proposal_id: String,
}

pub async fn steward_approve(
    Json(params): Json<ApproveParams>,
) -> Result<hk_core::steward::StewardProposal> {
    blocking(move || hk_core::steward::approve(&home_dir()?, &params.proposal_id)).await
}
