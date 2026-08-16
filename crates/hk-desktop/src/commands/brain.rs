use hk_core::{brain, steward, HkError};

fn home_dir() -> Result<std::path::PathBuf, HkError> {
    dirs::home_dir().ok_or_else(|| HkError::Internal("Cannot determine home directory".into()))
}

#[tauri::command]
pub fn brain_snapshot() -> Result<brain::BrainSnapshot, HkError> {
    brain::snapshot(&home_dir()?)
}

#[tauri::command]
pub fn steward_chat(
    prompt: String,
    history: Vec<steward::StewardChatMessage>,
) -> Result<steward::StewardChatReply, HkError> {
    steward::chat(&home_dir()?, &prompt, &history)
}

#[tauri::command]
pub fn steward_propose(prompt: String) -> Result<steward::StewardProposal, HkError> {
    steward::propose(&home_dir()?, &prompt)
}

#[tauri::command]
pub fn steward_propose_memory_edit(
    agent: String,
    path: String,
    content: String,
) -> Result<steward::StewardProposal, HkError> {
    steward::propose_memory_edit(&home_dir()?, &agent, &path, &content)
}

#[tauri::command]
pub fn steward_approve(proposal_id: String) -> Result<steward::StewardProposal, HkError> {
    steward::approve(&home_dir()?, &proposal_id)
}

#[tauri::command]
pub fn steward_reject(proposal_id: String) -> Result<steward::StewardProposal, HkError> {
    steward::reject(&home_dir()?, &proposal_id)
}
