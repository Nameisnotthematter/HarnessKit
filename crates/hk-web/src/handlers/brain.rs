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

#[derive(Deserialize)]
pub struct ChatParams {
    pub prompt: String,
    #[serde(default)]
    pub history: Vec<hk_core::steward::StewardChatMessage>,
}

pub async fn steward_chat(
    Json(params): Json<ChatParams>,
) -> Result<hk_core::steward::StewardChatReply> {
    blocking(move || hk_core::steward::chat(&home_dir()?, &params.prompt, &params.history)).await
}

pub async fn steward_propose(
    Json(params): Json<ProposeParams>,
) -> Result<hk_core::steward::StewardProposal> {
    blocking(move || hk_core::steward::propose(&home_dir()?, &params.prompt)).await
}

#[derive(Deserialize)]
pub struct MemoryEditParams {
    pub agent: String,
    pub path: String,
    pub content: String,
}

pub async fn steward_propose_memory_edit(
    Json(params): Json<MemoryEditParams>,
) -> Result<hk_core::steward::StewardProposal> {
    blocking(move || {
        hk_core::steward::propose_memory_edit(
            &home_dir()?,
            &params.agent,
            &params.path,
            &params.content,
        )
    })
    .await
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

pub async fn steward_reject(
    Json(params): Json<ApproveParams>,
) -> Result<hk_core::steward::StewardProposal> {
    blocking(move || hk_core::steward::reject(&home_dir()?, &params.proposal_id)).await
}
