use crate::adapter::{McpServerEntry, McpTransport};
use crate::brain;
use crate::deployer;
use crate::shared::{AgentMcpConfig, McpRegistry, McpServer};
use crate::HkError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const REGISTRY_RELATIVE: &str = ".harnesskit/shared/mcp-registry.yaml";
pub const MAX_MEMORY_EDIT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StewardValidation {
    pub label: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StewardProposal {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub diff: String,
    pub risk: String,
    pub validations: Vec<StewardValidation>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StewardChatRole {
    User,
    Steward,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StewardChatMessage {
    pub role: StewardChatRole,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StewardChatReply {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal: Option<StewardProposal>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ChangeAction {
    SkillsMigrate,
    McpToggle {
        server: String,
        agent: String,
        enabled: bool,
    },
    ConfigSet {
        agent: String,
        key: String,
        value: serde_json::Value,
    },
    PersonaReplace {
        agent: String,
        file: String,
        content: String,
    },
    MemoryReplace {
        agent: String,
        path: String,
        content: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredProposal {
    proposal: StewardProposal,
    actions: Vec<ChangeAction>,
    source_hashes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StewardConfig {
    base_url: String,
    model: String,
    #[serde(default = "default_api_key_env")]
    api_key_env: String,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    thinking: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelProposal {
    title: String,
    summary: String,
    risk: String,
    actions: Vec<ModelAction>,
}

struct ParsedChatContent {
    message: String,
    proposal: Option<ModelProposal>,
    proposal_error: Option<String>,
}

/// External models intentionally cannot deserialize memory actions. Memory
/// edits enter through `propose_memory_edit`, which validates an exact agent
/// and target path before creating the internal typed action.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ModelAction {
    SkillsMigrate,
    McpToggle {
        server: String,
        agent: String,
        enabled: bool,
    },
    ConfigSet {
        agent: String,
        key: String,
        value: serde_json::Value,
    },
    PersonaReplace {
        agent: String,
        file: String,
        content: String,
    },
}

impl From<ModelAction> for ChangeAction {
    fn from(action: ModelAction) -> Self {
        match action {
            ModelAction::SkillsMigrate => Self::SkillsMigrate,
            ModelAction::McpToggle {
                server,
                agent,
                enabled,
            } => Self::McpToggle {
                server,
                agent,
                enabled,
            },
            ModelAction::ConfigSet { agent, key, value } => Self::ConfigSet { agent, key, value },
            ModelAction::PersonaReplace {
                agent,
                file,
                content,
            } => Self::PersonaReplace {
                agent,
                file,
                content,
            },
        }
    }
}

fn default_api_key_env() -> String {
    "OPENAI_API_KEY".into()
}

pub fn list_proposals(home: &Path) -> Result<Vec<StewardProposal>, HkError> {
    let dir = proposals_dir(home);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(vec![]);
    };
    let mut proposals = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .filter_map(|entry| read_stored(&entry.path()).ok())
        .map(|stored| stored.proposal)
        .collect::<Vec<_>>();
    proposals.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(proposals)
}

pub fn propose(home: &Path, prompt: &str) -> Result<StewardProposal, HkError> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(HkError::Validation("Steward prompt cannot be empty".into()));
    }
    let model = match deterministic_model_proposal(prompt) {
        Some(model) => model,
        None => call_openai_compatible(home, prompt)?,
    };
    validate_model_proposal(home, &model)?;
    let actions = model.actions.into_iter().map(ChangeAction::from).collect();
    create_proposal(home, model.title, model.summary, model.risk, actions)
}

pub fn chat(
    home: &Path,
    prompt: &str,
    history: &[StewardChatMessage],
) -> Result<StewardChatReply, HkError> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(HkError::Validation("Steward prompt cannot be empty".into()));
    }

    if let Some(model) = deterministic_model_proposal(prompt) {
        validate_model_proposal(home, &model)?;
        let actions = model.actions.into_iter().map(ChangeAction::from).collect();
        let proposal = create_proposal(home, model.title, model.summary, model.risk, actions)?;
        return Ok(StewardChatReply {
            message: format!("I prepared a reviewable proposal: {}", proposal.title),
            proposal: Some(proposal),
        });
    }

    let content = call_openai_compatible_chat(home, prompt, history)?;
    let parsed = parse_chat_content(&content);
    let mut message = parsed.message;
    let proposal = match parsed.proposal {
        Some(model) => match validate_model_proposal(home, &model) {
            Ok(()) => {
                let actions = model.actions.into_iter().map(ChangeAction::from).collect();
                Some(create_proposal(
                    home,
                    model.title,
                    model.summary,
                    model.risk,
                    actions,
                )?)
            }
            Err(error) => {
                append_chat_notice(
                    &mut message,
                    &format!("I could not create that proposal safely: {error}"),
                );
                None
            }
        },
        None => {
            if let Some(error) = parsed.proposal_error {
                append_chat_notice(
                    &mut message,
                    &format!("I could not turn the suggested change into a proposal: {error}"),
                );
            }
            None
        }
    };
    if message.trim().is_empty() {
        message = "I reviewed the agent brains but did not receive a readable answer.".into();
    }
    Ok(StewardChatReply { message, proposal })
}

fn deterministic_model_proposal(prompt: &str) -> Option<ModelProposal> {
    let lower = prompt.to_ascii_lowercase();
    if lower.contains("skill")
        && (lower.contains("migrate") || lower.contains("迁移"))
    {
        return Some(ModelProposal {
            title: "Migrate personal Skills to the shared root".into(),
            summary: "Copy conflict-free personal Skills to ~/.agents/skills and configure Hermes/OpenClaw to load it.".into(),
            risk: "medium".into(),
            actions: vec![ModelAction::SkillsMigrate],
        });
    }
    parse_mcp_toggle(prompt).map(|action| ModelProposal {
        title: "Change MCP availability".into(),
        summary: "Update the secret-free registry and the selected runtime only.".into(),
        risk: "medium".into(),
        actions: vec![action],
    })
}

/// Creates a private memory replacement proposal. The file is not changed
/// until the returned proposal is passed to `approve`.
pub fn propose_memory_edit(
    home: &Path,
    agent: &str,
    path: &str,
    content: &str,
) -> Result<StewardProposal, HkError> {
    if content.len() > MAX_MEMORY_EDIT_BYTES {
        return Err(HkError::Validation("memory content is too large".into()));
    }
    let target = memory_path(home, agent, path)?;
    create_proposal(
        home,
        format!("Edit private {agent} memory"),
        format!("Replace {} after explicit approval.", target.display()),
        "high".into(),
        vec![ChangeAction::MemoryReplace {
            agent: agent.into(),
            path: target.to_string_lossy().into_owned(),
            content: content.into(),
        }],
    )
}

fn create_proposal(
    home: &Path,
    title: String,
    summary: String,
    risk: String,
    actions: Vec<ChangeAction>,
) -> Result<StewardProposal, HkError> {
    let source_hashes = source_hashes(home, &actions)?;
    let proposal = StewardProposal {
        id: uuid::Uuid::new_v4().to_string(),
        title,
        summary,
        diff: render_diff(&actions),
        risk: normalize_risk(&risk).into(),
        validations: vec![
            StewardValidation {
                label: "Typed action schema".into(),
                status: "pass".into(),
                detail: Some(
                    "Only known config, persona, MCP, and explicit memory actions are accepted"
                        .into(),
                ),
            },
            StewardValidation {
                label: "Optimistic concurrency".into(),
                status: "pass".into(),
                detail: Some("Every target hash is rechecked before apply".into()),
            },
            StewardValidation {
                label: "Memory isolation".into(),
                status: "pass".into(),
                detail: Some(
                    "Memory is omitted from model context and edits require explicit approval"
                        .into(),
                ),
            },
        ],
        status: "pending".into(),
        created_at: Utc::now(),
    };
    let stored = StoredProposal {
        proposal: proposal.clone(),
        actions,
        source_hashes,
    };
    save_stored(home, &stored)?;
    append_audit(home, &proposal.id, "proposed", "proposal created")?;
    Ok(proposal)
}

pub fn approve(home: &Path, proposal_id: &str) -> Result<StewardProposal, HkError> {
    validate_id(proposal_id)?;
    let path = proposal_path(home, proposal_id);
    let mut stored = read_stored(&path)?;
    if stored.proposal.status != "pending" {
        return Err(HkError::Conflict(format!(
            "proposal {} is already {}",
            proposal_id, stored.proposal.status
        )));
    }
    verify_hashes(&stored.source_hashes)?;

    let targets = action_targets(home, &stored.actions)?;
    let backup_dir = home.join(".harnesskit/backups").join(&stored.proposal.id);
    let backups = backup_targets(&targets, &backup_dir)?;
    let result = apply_actions(home, &stored.actions).and_then(|_| validate_targets(&targets));
    match result {
        Ok(()) => {
            stored.proposal.status = "approved".into();
            save_stored(home, &stored)?;
            append_audit(home, proposal_id, "approved", "proposal applied")?;
            Ok(stored.proposal)
        }
        Err(error) => {
            let rollback = restore_backups(&backups);
            stored.proposal.status = "failed".into();
            save_stored(home, &stored)?;
            append_audit(home, proposal_id, "failed", &error.to_string())?;
            if let Err(rollback_error) = rollback {
                return Err(HkError::Internal(format!(
                    "apply failed: {error}; rollback failed: {rollback_error}"
                )));
            }
            Err(error)
        }
    }
}

pub fn reject(home: &Path, proposal_id: &str) -> Result<StewardProposal, HkError> {
    validate_id(proposal_id)?;
    let path = proposal_path(home, proposal_id);
    let mut stored = read_stored(&path)?;
    if stored.proposal.status != "pending" {
        return Err(HkError::Conflict(format!(
            "proposal {} is already {}",
            proposal_id, stored.proposal.status
        )));
    }

    stored.proposal.status = "rejected".into();
    save_stored(home, &stored)?;
    append_audit(
        home,
        proposal_id,
        "rejected",
        "proposal rejected without applying actions",
    )?;
    Ok(stored.proposal)
}

fn parse_mcp_toggle(prompt: &str) -> Option<ModelAction> {
    let lower = prompt.to_ascii_lowercase();
    if !lower.contains("mcp") {
        return None;
    }
    let command = lower.trim_start();
    let enabled = if command.starts_with("disable ")
        || command.starts_with("please disable ")
        || command.starts_with("关闭")
        || command.starts_with("禁用")
    {
        false
    } else if command.starts_with("enable ")
        || command.starts_with("please enable ")
        || command.starts_with("开启")
        || command.starts_with("启用")
    {
        true
    } else {
        return None;
    };
    let agent = ["codex", "hermes", "openclaw"]
        .into_iter()
        .find(|agent| lower.contains(agent))?;
    let server = quoted_value(prompt).or_else(|| {
        let words = prompt.split_whitespace().collect::<Vec<_>>();
        let mcp = words
            .iter()
            .position(|word| word.eq_ignore_ascii_case("mcp"))?;
        words.get(mcp + 2).map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                .to_string()
        })
    })?;
    if server.is_empty() {
        return None;
    }
    Some(ModelAction::McpToggle {
        server,
        agent: agent.into(),
        enabled,
    })
}

fn quoted_value(text: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let start = text.find(quote)?;
        let rest = &text[start + quote.len_utf8()..];
        let end = rest.find(quote)?;
        return Some(rest[..end].to_string());
    }
    None
}

fn call_openai_compatible(home: &Path, prompt: &str) -> Result<ModelProposal, HkError> {
    let config_path = home.join(".harnesskit/steward/config.yaml");
    let raw = fs::read_to_string(&config_path).map_err(|_| {
        HkError::Validation(format!(
            "Steward backend is not configured; create {} with base_url, model, and api_key_env",
            config_path.display()
        ))
    })?;
    let config: StewardConfig =
        serde_yaml::from_str(&raw).map_err(|error| HkError::ConfigCorrupted(error.to_string()))?;
    if config.base_url.trim().is_empty() || config.model.trim().is_empty() {
        return Err(HkError::Validation(
            "Steward base_url and model are required".into(),
        ));
    }
    let safe_context = safe_model_context(brain::snapshot(home)?);
    let system = format!(
        "You are HarnessKit Brain Steward. Return JSON only with title, summary, risk, actions. \
         Allowed actions: {{\"type\":\"skills_migrate\"}}, {{\"type\":\"mcp_toggle\",\"server\":string,\"agent\":\"codex|hermes|openclaw\",\"enabled\":bool}}, \
         {{\"type\":\"config_set\",\"agent\":...,\"key\":string,\"value\":json}}, \
         {{\"type\":\"persona_replace\",\"agent\":...,\"file\":string,\"content\":string}}. \
         Never propose memory edits. Do not include secrets. Current safe snapshot: {safe_context}"
    );
    let body = build_chat_body(&config, system, prompt)?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let mut request = client.post(url).json(&body);
    if let Ok(api_key) = std::env::var(&config.api_key_env) {
        request = request.bearer_auth(api_key);
    }
    let response = request.send()?.error_for_status()?;
    let payload: serde_json::Value = response.json()?;
    let content = payload
        .pointer("/choices/0/message/content")
        .and_then(|value| value.as_str())
        .ok_or_else(|| HkError::ConfigCorrupted("model response has no message content".into()))?;
    serde_json::from_str(content).map_err(|error| {
        HkError::ConfigCorrupted(format!("model returned invalid proposal JSON: {error}"))
    })
}

fn call_openai_compatible_chat(
    home: &Path,
    prompt: &str,
    history: &[StewardChatMessage],
) -> Result<String, HkError> {
    let config_path = home.join(".harnesskit/steward/config.yaml");
    let raw = fs::read_to_string(&config_path).map_err(|_| {
        HkError::Validation(format!(
            "Steward backend is not configured; create {} with base_url, model, and api_key_env",
            config_path.display()
        ))
    })?;
    let config: StewardConfig =
        serde_yaml::from_str(&raw).map_err(|error| HkError::ConfigCorrupted(error.to_string()))?;
    if config.base_url.trim().is_empty() || config.model.trim().is_empty() {
        return Err(HkError::Validation(
            "Steward base_url and model are required".into(),
        ));
    }
    let safe_context = safe_model_context(brain::snapshot(home)?);
    let system = format!(
        "You are HarnessKit Brain Steward. Talk naturally and answer ordinary questions directly. \
         You can inspect the current redacted agent-brain snapshot below, including agent configs, \
         personas, shared Skills, and MCP state. Never claim that a file was changed unless an \
         approved proposal was applied. Never reveal or request secrets. Memory content is private \
         and is not present in this context. If and only if the user asks to mutate an agent brain, \
         explain the change naturally and append one <harnesskit-proposal>...</harnesskit-proposal> \
         block containing JSON with title, summary, risk, and actions. Allowed actions: \
         {{\"type\":\"skills_migrate\"}}, \
         {{\"type\":\"mcp_toggle\",\"server\":string,\"agent\":\"codex|hermes|openclaw\",\"enabled\":bool}}, \
         {{\"type\":\"config_set\",\"agent\":...,\"key\":string,\"value\":json}}, \
         {{\"type\":\"persona_replace\",\"agent\":...,\"file\":string,\"content\":string}}. \
         Never propose memory edits in chat. Current safe snapshot: {safe_context}"
    );
    let body = build_free_chat_body(&config, system, history, prompt)?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let mut request = client.post(url).json(&body);
    if let Ok(api_key) = std::env::var(&config.api_key_env) {
        request = request.bearer_auth(api_key);
    }
    let response = request.send()?.error_for_status()?;
    let payload: serde_json::Value = response.json()?;
    payload
        .pointer("/choices/0/message/content")
        .and_then(|value| value.as_str())
        .map(String::from)
        .ok_or_else(|| HkError::ConfigCorrupted("model response has no message content".into()))
}

fn parse_chat_content(content: &str) -> ParsedChatContent {
    const START: &str = "<harnesskit-proposal>";
    const END: &str = "</harnesskit-proposal>";
    let Some(start) = content.find(START) else {
        return ParsedChatContent {
            message: content.trim().into(),
            proposal: None,
            proposal_error: None,
        };
    };
    let proposal_start = start + START.len();
    let Some(relative_end) = content[proposal_start..].find(END) else {
        return ParsedChatContent {
            message: content.trim().into(),
            proposal: None,
            proposal_error: None,
        };
    };
    let end = proposal_start + relative_end;
    let before = content[..start].trim();
    let after = content[end + END.len()..].trim();
    let message = match (before.is_empty(), after.is_empty()) {
        (false, false) => format!("{before}\n{after}"),
        (false, true) => before.into(),
        (true, false) => after.into(),
        (true, true) => String::new(),
    };
    let raw_proposal = content[proposal_start..end].trim();
    let raw_proposal = raw_proposal
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(raw_proposal);
    match serde_json::from_str(raw_proposal) {
        Ok(proposal) => ParsedChatContent {
            message,
            proposal: Some(proposal),
            proposal_error: None,
        },
        Err(error) => ParsedChatContent {
            message,
            proposal: None,
            proposal_error: Some(error.to_string()),
        },
    }
}

fn append_chat_notice(message: &mut String, notice: &str) {
    if !message.trim().is_empty() {
        message.push_str("\n\n");
    }
    message.push_str(notice);
}

fn build_chat_body(
    config: &StewardConfig,
    system: String,
    prompt: &str,
) -> Result<serde_json::Value, HkError> {
    let mut body = serde_json::json!({
        "model": config.model,
        "temperature": 0,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": prompt}
        ]
    });
    if let Some(effort) = config
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
    {
        if !matches!(effort, "low" | "medium" | "high" | "max") {
            return Err(HkError::Validation(format!(
                "unsupported reasoning_effort {effort:?}; expected low, medium, high, or max"
            )));
        }
        body["reasoning_effort"] = effort.into();
    }
    if config.thinking {
        body["thinking"] = serde_json::json!({"type": "enabled"});
    }
    Ok(body)
}

fn build_free_chat_body(
    config: &StewardConfig,
    system: String,
    history: &[StewardChatMessage],
    prompt: &str,
) -> Result<serde_json::Value, HkError> {
    let mut messages = vec![serde_json::json!({"role": "system", "content": system})];
    messages.extend(history.iter().filter_map(|message| {
        let content = message.content.trim();
        if content.is_empty() {
            return None;
        }
        let role = match message.role {
            StewardChatRole::User => "user",
            StewardChatRole::Steward => "assistant",
        };
        Some(serde_json::json!({"role": role, "content": content}))
    }));
    messages.push(serde_json::json!({"role": "user", "content": prompt}));
    let mut body = serde_json::json!({
        "model": config.model,
        "temperature": 0,
        "messages": messages,
    });
    if let Some(effort) = config
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
    {
        if !matches!(effort, "low" | "medium" | "high" | "max") {
            return Err(HkError::Validation(format!(
                "unsupported reasoning_effort {effort:?}; expected low, medium, high, or max"
            )));
        }
        body["reasoning_effort"] = effort.into();
    }
    if config.thinking {
        body["thinking"] = serde_json::json!({"type": "enabled"});
    }
    Ok(body)
}

fn safe_model_context(snapshot: brain::BrainSnapshot) -> serde_json::Value {
    serde_json::json!({
        "agents": snapshot.agents.into_iter().map(|agent| serde_json::json!({
            "id": agent.id,
            "status": agent.status,
            "config": agent.config,
            "persona": agent.persona,
        })).collect::<Vec<_>>(),
        "shared_skills": snapshot.shared_skills,
        "mcp_registry": snapshot.mcp_registry,
        "memory_omitted": true,
    })
}

fn validate_model_proposal(home: &Path, model: &ModelProposal) -> Result<(), HkError> {
    if model.title.trim().is_empty() || model.summary.trim().is_empty() || model.actions.is_empty()
    {
        return Err(HkError::Validation(
            "proposal must have title, summary, and at least one action".into(),
        ));
    }
    if model.actions.len() > 8 {
        return Err(HkError::Validation("proposal has too many actions".into()));
    }
    for action in &model.actions {
        match action {
            ModelAction::SkillsMigrate => {
                let plan = shared_skill_plan(home)?;
                if plan
                    .items
                    .iter()
                    .any(|item| item.decision == crate::shared::SkillPlanDecision::Conflict)
                {
                    return Err(HkError::Conflict(
                        "shared Skill migration has unresolved name conflicts".into(),
                    ));
                }
            }
            ModelAction::McpToggle { server, agent, .. } => {
                validate_agent(agent)?;
                if server.is_empty() || server.contains(['/', '\\']) {
                    return Err(HkError::Validation("invalid MCP server name".into()));
                }
            }
            ModelAction::ConfigSet { agent, key, .. } => {
                validate_agent(agent)?;
                if !known_config_keys(agent).contains(&key.as_str()) {
                    return Err(HkError::Validation(format!(
                        "{agent} config key {key:?} is not editable"
                    )));
                }
            }
            ModelAction::PersonaReplace {
                agent,
                file,
                content,
            } => {
                validate_agent(agent)?;
                persona_path(Path::new("/"), agent, file)?;
                if content.len() > MAX_MEMORY_EDIT_BYTES {
                    return Err(HkError::Validation("persona content is too large".into()));
                }
            }
        }
    }
    Ok(())
}

fn source_hashes(
    home: &Path,
    actions: &[ChangeAction],
) -> Result<BTreeMap<String, String>, HkError> {
    let mut hashes = BTreeMap::new();
    for target in action_targets(home, actions)? {
        hashes.insert(target.to_string_lossy().into_owned(), hash_path(&target)?);
    }
    if actions
        .iter()
        .any(|action| matches!(action, ChangeAction::SkillsMigrate))
    {
        for source in shared_skill_sources(home) {
            for skill in crate::shared::inventory_skills(&source.root)
                .map_err(|error| HkError::Internal(error.to_string()))?
            {
                hashes.insert(
                    skill.path.to_string_lossy().into_owned(),
                    hash_path(&skill.path)?,
                );
            }
        }
    }
    Ok(hashes)
}

fn action_targets(home: &Path, actions: &[ChangeAction]) -> Result<Vec<PathBuf>, HkError> {
    let mut paths = Vec::new();
    for action in actions {
        match action {
            ChangeAction::SkillsMigrate => {
                paths.push(home.join(".agents/skills"));
                paths.push(home.join(".hermes/config.yaml"));
                paths.push(home.join(".openclaw/openclaw.json"));
            }
            ChangeAction::McpToggle { agent, .. } => {
                paths.push(home.join(REGISTRY_RELATIVE));
                paths.push(config_path(home, agent)?);
            }
            ChangeAction::ConfigSet { agent, .. } => paths.push(config_path(home, agent)?),
            ChangeAction::PersonaReplace { agent, file, .. } => {
                paths.push(persona_path(home, agent, file)?)
            }
            ChangeAction::MemoryReplace { agent, path, .. } => {
                paths.push(memory_path(home, agent, path)?)
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn verify_hashes(hashes: &BTreeMap<String, String>) -> Result<(), HkError> {
    for (path, expected) in hashes {
        let actual = hash_path(Path::new(path))?;
        if &actual != expected {
            return Err(HkError::Conflict(format!(
                "{} changed after proposal creation",
                path
            )));
        }
    }
    Ok(())
}

fn apply_actions(home: &Path, actions: &[ChangeAction]) -> Result<(), HkError> {
    for action in actions {
        match action {
            ChangeAction::SkillsMigrate => apply_skills_migration(home)?,
            ChangeAction::McpToggle {
                server,
                agent,
                enabled,
            } => apply_mcp_toggle(home, server, agent, *enabled)?,
            ChangeAction::ConfigSet { agent, key, value } => {
                apply_config_set(home, agent, key, value)?
            }
            ChangeAction::PersonaReplace {
                agent,
                file,
                content,
            } => atomic_write(&persona_path(home, agent, file)?, content.as_bytes())?,
            ChangeAction::MemoryReplace {
                agent,
                path,
                content,
            } => {
                if content.len() > MAX_MEMORY_EDIT_BYTES {
                    return Err(HkError::Validation("memory content is too large".into()));
                }
                atomic_write(&memory_path(home, agent, path)?, content.as_bytes())?
            }
        }
    }
    Ok(())
}

fn shared_skill_sources(home: &Path) -> Vec<crate::shared::SkillSource> {
    vec![
        crate::shared::SkillSource {
            agent: "codex".into(),
            root: home.join(".codex/skills"),
        },
        crate::shared::SkillSource {
            agent: "hermes".into(),
            root: home.join(".hermes/skills/local"),
        },
        crate::shared::SkillSource {
            agent: "openclaw".into(),
            root: home.join(".openclaw/skills"),
        },
    ]
}

fn shared_skill_plan(home: &Path) -> Result<crate::shared::SkillMigrationPlan, HkError> {
    crate::shared::plan_skill_migration(&home.join(".agents/skills"), &shared_skill_sources(home))
        .map_err(|error| HkError::Internal(error.to_string()))
}

fn apply_skills_migration(home: &Path) -> Result<(), HkError> {
    let plan = shared_skill_plan(home)?;
    let conflicts = plan
        .items
        .iter()
        .filter(|item| item.decision == crate::shared::SkillPlanDecision::Conflict)
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();
    if !conflicts.is_empty() {
        return Err(HkError::Conflict(format!(
            "resolve shared Skill conflicts first: {}",
            conflicts.join(", ")
        )));
    }
    fs::create_dir_all(&plan.canonical_root)?;
    for item in plan.items {
        if item.decision != crate::shared::SkillPlanDecision::Add {
            continue;
        }
        let source = item
            .sources
            .first()
            .ok_or_else(|| HkError::Internal(format!("Skill {} has no source", item.name)))?;
        deployer::deploy_skill(&source.path, &plan.canonical_root)?;
    }
    configure_shared_skill_root(home)
}

fn configure_shared_skill_root(home: &Path) -> Result<(), HkError> {
    let shared = home.join(".agents/skills").to_string_lossy().into_owned();

    let hermes_path = home.join(".hermes/config.yaml");
    let hermes_raw = fs::read_to_string(&hermes_path).unwrap_or_default();
    let mut hermes: serde_yaml::Value = if hermes_raw.trim().is_empty() {
        serde_yaml::Value::Mapping(Default::default())
    } else {
        serde_yaml::from_str(&hermes_raw)
            .map_err(|error| HkError::ConfigCorrupted(error.to_string()))?
    };
    let root = hermes
        .as_mapping_mut()
        .ok_or_else(|| HkError::ConfigCorrupted("Hermes root is not a mapping".into()))?;
    let skills = root
        .entry("skills".into())
        .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()))
        .as_mapping_mut()
        .ok_or_else(|| HkError::ConfigCorrupted("Hermes skills is not a mapping".into()))?;
    let dirs = skills
        .entry("external_dirs".into())
        .or_insert_with(|| serde_yaml::Value::Sequence(vec![]))
        .as_sequence_mut()
        .ok_or_else(|| HkError::ConfigCorrupted("skills.external_dirs is not a list".into()))?;
    if !dirs.iter().any(|value| value.as_str() == Some(&shared)) {
        dirs.push(shared.clone().into());
    }
    atomic_write(
        &hermes_path,
        serde_yaml::to_string(&hermes)
            .map_err(|error| HkError::Internal(error.to_string()))?
            .as_bytes(),
    )?;

    let openclaw_path = home.join(".openclaw/openclaw.json");
    let openclaw_raw = fs::read_to_string(&openclaw_path).unwrap_or_else(|_| "{}".into());
    let mut openclaw: serde_json::Value = serde_json::from_str(&openclaw_raw)?;
    let root = openclaw
        .as_object_mut()
        .ok_or_else(|| HkError::ConfigCorrupted("OpenClaw root is not an object".into()))?;
    let skills = root
        .entry("skills")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| HkError::ConfigCorrupted("OpenClaw skills is not an object".into()))?;
    let load = skills
        .entry("load")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| HkError::ConfigCorrupted("OpenClaw skills.load is not an object".into()))?;
    let dirs = load
        .entry("extraDirs")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| HkError::ConfigCorrupted("skills.load.extraDirs is not a list".into()))?;
    if !dirs.iter().any(|value| value.as_str() == Some(&shared)) {
        dirs.push(shared.into());
    }
    atomic_write(
        &openclaw_path,
        (serde_json::to_string_pretty(&openclaw)? + "\n").as_bytes(),
    )
}

fn apply_mcp_toggle(
    home: &Path,
    server_name: &str,
    agent_name: &str,
    enabled: bool,
) -> Result<(), HkError> {
    let adapters = brain::adapters_for_home(home);
    let target = adapters
        .iter()
        .find(|adapter| adapter.name() == agent_name)
        .ok_or_else(|| HkError::NotFound(format!("agent {agent_name}")))?;
    let registry_path = home.join(REGISTRY_RELATIVE);
    let mut registry = if registry_path.exists() {
        crate::shared::load_mcp_registry(&registry_path)
            .map_err(|error| HkError::ConfigCorrupted(error.to_string()))?
    } else {
        McpRegistry {
            version: 1,
            servers: BTreeMap::new(),
            agents: BTreeMap::new(),
        }
    };
    if !registry.servers.contains_key(server_name) {
        let discovered = adapters
            .iter()
            .flat_map(|adapter| adapter.read_mcp_servers())
            .find(|entry| entry.name == server_name)
            .ok_or_else(|| {
                HkError::NotFound(format!(
                    "MCP server {server_name:?} is not in the registry or native configs"
                ))
            })?;
        registry
            .servers
            .insert(server_name.into(), registry_server(&discovered));
    }
    registry
        .agents
        .entry(agent_name.into())
        .or_insert_with(AgentMcpConfig::default)
        .enabled
        .insert(server_name.into(), enabled);
    registry
        .validate()
        .map_err(|error| HkError::Validation(error.to_string()))?;
    let yaml =
        serde_yaml::to_string(&registry).map_err(|error| HkError::Internal(error.to_string()))?;
    atomic_write(&registry_path, yaml.as_bytes())?;

    let existing = target
        .read_mcp_servers()
        .into_iter()
        .find(|entry| entry.name == server_name);
    if target.supports_native_mcp_toggle() && existing.is_some() {
        match agent_name {
            "hermes" => {
                deployer::set_hermes_mcp_enabled(&target.mcp_config_path(), server_name, enabled)
            }
            "openclaw" => {
                deployer::set_openclaw_mcp_enabled(&target.mcp_config_path(), server_name, enabled)
            }
            _ => Err(HkError::Internal(format!(
                "native MCP toggle is not implemented for {agent_name}"
            ))),
        }?;
    } else if enabled && existing.is_none() {
        let server = registry
            .plan_for_agent(agent_name)
            .map_err(|error| HkError::Validation(error.to_string()))?
            .servers
            .get(server_name)
            .cloned()
            .ok_or_else(|| HkError::NotFound(format!("registry server {server_name}")))?;
        let entry = runtime_entry(server_name, &server)?;
        deployer::deploy_mcp_server(&target.mcp_config_path(), &entry, target.as_ref())?;
    } else if !enabled && existing.is_some() {
        deployer::remove_mcp_server(&target.mcp_config_path(), server_name, target.mcp_format())?;
    }
    Ok(())
}

fn registry_server(entry: &McpServerEntry) -> McpServer {
    McpServer {
        enabled_by_default: false,
        command: (entry.transport == McpTransport::Stdio).then(|| entry.command.clone()),
        args: entry.args.clone(),
        cwd: None,
        url: entry.url.clone(),
        transport: Some(entry.transport.as_str().into()),
        env_keys: entry.env.keys().cloned().collect(),
        env_secret_refs: BTreeMap::new(),
        header_secret_refs: BTreeMap::new(),
        tool_allow: vec![],
    }
}

fn runtime_entry(name: &str, server: &McpServer) -> Result<McpServerEntry, HkError> {
    let env = server
        .env_keys
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.clone(), value)))
        .chain(
            server
                .env_secret_refs
                .iter()
                .map(|(key, reference)| {
                    resolve_secret_ref(reference).map(|value| (key.clone(), value))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
        .collect();
    let headers = server
        .header_secret_refs
        .iter()
        .map(|(key, reference)| resolve_secret_ref(reference).map(|value| (key.clone(), value)))
        .collect::<Result<_, _>>()?;
    let transport = match server.transport.as_deref() {
        Some("sse") => McpTransport::Sse,
        Some("http") => McpTransport::Http,
        _ if server.url.is_some() => McpTransport::Http,
        _ => McpTransport::Stdio,
    };
    Ok(McpServerEntry {
        name: name.into(),
        command: server.command.clone().unwrap_or_default(),
        args: server.args.clone(),
        env,
        transport,
        url: server.url.clone(),
        headers,
        enabled: true,
    })
}

fn resolve_secret_ref(reference: &str) -> Result<String, HkError> {
    let key = reference
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| HkError::Validation("invalid secret reference".into()))?;
    std::env::var(key)
        .map_err(|_| HkError::Validation(format!("required environment variable {key} is unset")))
}

fn apply_config_set(
    home: &Path,
    agent: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), HkError> {
    if !known_config_keys(agent).contains(&key) {
        return Err(HkError::Validation(format!(
            "{agent} config key {key:?} is not editable"
        )));
    }
    let path = config_path(home, agent)?;
    let raw = fs::read_to_string(&path).unwrap_or_default();
    let output = match agent {
        "codex" => {
            let mut doc = raw.parse::<toml::Table>().unwrap_or_default();
            set_toml_path(&mut doc, key, json_to_toml(value)?)?;
            toml::to_string_pretty(&doc).map_err(|error| HkError::Internal(error.to_string()))?
        }
        "hermes" => {
            let mut doc = if raw.trim().is_empty() {
                serde_yaml::Value::Mapping(Default::default())
            } else {
                serde_yaml::from_str(&raw)
                    .map_err(|error| HkError::ConfigCorrupted(error.to_string()))?
            };
            set_yaml_path(&mut doc, key, json_to_yaml(value)?)?;
            serde_yaml::to_string(&doc).map_err(|error| HkError::Internal(error.to_string()))?
        }
        "openclaw" => {
            let mut doc = if raw.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&raw)?
            };
            set_json_path(&mut doc, key, value.clone())?;
            serde_json::to_string_pretty(&doc)? + "\n"
        }
        _ => return Err(HkError::Validation(format!("unknown agent {agent}"))),
    };
    atomic_write(&path, output.as_bytes())
}

fn known_config_keys(agent: &str) -> &'static [&'static str] {
    match agent {
        "codex" => &[
            "model",
            "model_reasoning_effort",
            "personality",
            "sandbox_mode",
            "approval_policy",
            "features.memories",
        ],
        "hermes" => &[
            "model",
            "provider",
            "agent.reasoning_effort",
            "agent.max_turns",
            "agent.system_prompt",
            "display.personality",
            "memory.enabled",
            "memory.user_profile_enabled",
        ],
        "openclaw" => &[
            "agents.defaults.model.primary",
            "agents.defaults.workspace",
            "agents.defaults.heartbeat.every",
            "tools.profile",
            "browser.enabled",
        ],
        _ => &[],
    }
}

fn config_path(home: &Path, agent: &str) -> Result<PathBuf, HkError> {
    match agent {
        "codex" => Ok(home.join(".codex/config.toml")),
        "hermes" => Ok(home.join(".hermes/config.yaml")),
        "openclaw" => Ok(home.join(".openclaw/openclaw.json")),
        _ => Err(HkError::Validation(format!("unknown agent {agent}"))),
    }
}

fn persona_path(home: &Path, agent: &str, file: &str) -> Result<PathBuf, HkError> {
    let allowed: &[&str] = match agent {
        "codex" => &["AGENTS.md"],
        "hermes" => &["SOUL.md"],
        "openclaw" => &[
            "AGENTS.md",
            "SOUL.md",
            "IDENTITY.md",
            "USER.md",
            "TOOLS.md",
            "HEARTBEAT.md",
            "DREAMS.md",
        ],
        _ => return Err(HkError::Validation(format!("unknown agent {agent}"))),
    };
    if !allowed.contains(&file) {
        return Err(HkError::Validation(format!(
            "{file:?} is not an editable {agent} persona file"
        )));
    }
    Ok(match agent {
        "codex" => home.join(".codex").join(file),
        "hermes" => home.join(".hermes").join(file),
        "openclaw" => openclaw_workspace(home)?.join(file),
        _ => unreachable!(),
    })
}

fn openclaw_workspace(home: &Path) -> Result<PathBuf, HkError> {
    let config = home.join(".openclaw/openclaw.json");
    let raw = fs::read_to_string(config).unwrap_or_default();
    let configured = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|doc| {
            doc.pointer("/agents/defaults/workspace")
                .and_then(|value| value.as_str())
                .map(String::from)
        });
    let workspace = match configured {
        Some(path) if path.starts_with("~/") => home.join(path.trim_start_matches("~/")),
        Some(path) if Path::new(&path).is_absolute() => PathBuf::from(path),
        _ => home.join(".openclaw/workspace"),
    };
    if workspace.starts_with(home.join(".codex")) || workspace.starts_with(home.join(".hermes")) {
        return Err(HkError::Validation(
            "OpenClaw workspace cannot overlap another agent's private directory".into(),
        ));
    }
    Ok(workspace)
}

fn memory_path(home: &Path, agent: &str, requested: &str) -> Result<PathBuf, HkError> {
    validate_agent(agent)?;
    let target = PathBuf::from(requested);
    if !target.is_absolute()
        || target.components().any(|component| {
            !matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
    {
        return Err(HkError::Validation(
            "memory path must be an absolute path without traversal".into(),
        ));
    }

    let (root, standalone, recursive, markdown_only) = match agent {
        "codex" => (home.join(".codex/memories"), None, false, true),
        "hermes" => (home.join(".hermes/memories"), None, false, false),
        "openclaw" => {
            let workspace = openclaw_workspace(home)?;
            (
                workspace.join("memory"),
                Some(workspace.join("MEMORY.md")),
                true,
                true,
            )
        }
        _ => unreachable!(),
    };
    let in_root = if recursive {
        target.starts_with(&root) && target != root
    } else {
        target.parent() == Some(root.as_path())
    };
    if !in_root && standalone.as_ref() != Some(&target) {
        return Err(HkError::Validation(format!(
            "memory path does not belong to {agent}"
        )));
    }
    if markdown_only && target.extension().and_then(|ext| ext.to_str()) != Some("md") {
        return Err(HkError::Validation(format!(
            "{agent} memory target must be a Markdown file"
        )));
    }

    let symlink_anchor = if standalone.as_deref() == Some(target.as_path()) {
        target.parent().unwrap_or(&root)
    } else {
        &root
    };
    reject_symlink_components(symlink_anchor, &target)?;
    if target.exists() {
        let adapter = brain::adapters_for_home(home)
            .into_iter()
            .find(|adapter| adapter.name() == agent)
            .ok_or_else(|| HkError::Validation(format!("unknown agent {agent}")))?;
        if !adapter.global_memory_files().contains(&target) {
            return Err(HkError::Validation(format!(
                "memory path is not discovered for {agent}"
            )));
        }
    }
    Ok(target)
}

fn reject_symlink_components(root: &Path, target: &Path) -> Result<(), HkError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| HkError::Validation("memory path is outside its memory root".into()))?;
    let mut current = root.to_path_buf();
    if fs::symlink_metadata(&current).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(HkError::Validation(format!(
            "memory root is a symlink: {}",
            current.display()
        )));
    }
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(HkError::Validation(format!(
                    "memory path crosses symlink {}",
                    current.display()
                )));
            }
            Ok(metadata) if current != target && !metadata.is_dir() => {
                return Err(HkError::Validation(format!(
                    "memory path parent is not a directory: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn validate_agent(agent: &str) -> Result<(), HkError> {
    if matches!(agent, "codex" | "hermes" | "openclaw") {
        Ok(())
    } else {
        Err(HkError::Validation(format!("unknown agent {agent}")))
    }
}

fn render_diff(actions: &[ChangeAction]) -> String {
    actions
        .iter()
        .map(|action| match action {
            ChangeAction::SkillsMigrate => {
                "+ shared Skills: migrate conflict-free personal skills to ~/.agents/skills".into()
            }
            ChangeAction::McpToggle {
                server,
                agent,
                enabled,
            } => format!(
                "~ mcp.registry.{server}.agents.{agent}: {}",
                if *enabled { "enabled" } else { "disabled" }
            ),
            ChangeAction::ConfigSet { agent, key, value } => {
                format!("~ {agent}.config.{key}: {value}")
            }
            ChangeAction::PersonaReplace {
                agent,
                file,
                content,
            } => format!(
                "~ {agent}.persona.{file}: replace with {} bytes",
                content.len()
            ),
            ChangeAction::MemoryReplace {
                agent,
                path,
                content,
            } => format!(
                "~ {agent}.memory.{path}: replace with {} bytes",
                content.len()
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_risk(risk: &str) -> &str {
    match risk.to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

fn proposals_dir(home: &Path) -> PathBuf {
    home.join(".harnesskit/steward/proposals")
}

fn proposal_path(home: &Path, id: &str) -> PathBuf {
    proposals_dir(home).join(format!("{id}.json"))
}

fn validate_id(id: &str) -> Result<(), HkError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| HkError::Validation("invalid proposal id".into()))
}

fn read_stored(path: &Path) -> Result<StoredProposal, HkError> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(Into::into)
}

fn save_stored(home: &Path, stored: &StoredProposal) -> Result<(), HkError> {
    validate_id(&stored.proposal.id)?;
    let output = serde_json::to_vec_pretty(stored)?;
    atomic_write(&proposal_path(home, &stored.proposal.id), &output)
}

fn hash_path(path: &Path) -> Result<String, HkError> {
    if !path.exists() {
        return Ok("missing".into());
    }
    if path.is_dir() {
        let mut files = walkdir::WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();
        files.sort();
        let mut hasher = Sha256::new();
        for file in files {
            let relative = file.strip_prefix(path).unwrap_or(&file).to_string_lossy();
            hasher.update(relative.as_bytes());
            hasher.update(fs::read(file)?);
        }
        return Ok(format!("{:x}", hasher.finalize()));
    }
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), HkError> {
    let parent = path
        .parent()
        .ok_or_else(|| HkError::Validation("target has no parent directory".into()))?;
    let existing_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    if let Some(permissions) = existing_permissions {
        fs::set_permissions(temp.path(), permissions)?;
    }
    temp.persist(path)
        .map_err(|error| HkError::Internal(error.error.to_string()))?;
    Ok(())
}

fn backup_targets(
    targets: &[PathBuf],
    backup_dir: &Path,
) -> Result<BTreeMap<PathBuf, Option<PathBuf>>, HkError> {
    fs::create_dir_all(backup_dir)?;
    let mut backups = BTreeMap::new();
    for (index, target) in targets.iter().enumerate() {
        if target.exists() {
            let backup = if target.is_dir() {
                let backup = backup_dir.join(format!("{index}.dir"));
                copy_directory(target, &backup)?;
                backup
            } else {
                let backup = backup_dir.join(format!("{index}.bak"));
                fs::copy(target, &backup)?;
                backup
            };
            backups.insert(target.clone(), Some(backup));
        } else {
            backups.insert(target.clone(), None);
        }
    }
    Ok(backups)
}

fn restore_backups(backups: &BTreeMap<PathBuf, Option<PathBuf>>) -> Result<(), HkError> {
    for (target, backup) in backups {
        match backup {
            Some(backup) => {
                if backup.is_dir() {
                    if target.exists() {
                        fs::remove_dir_all(target)?;
                    }
                    copy_directory(backup, target)?;
                } else {
                    fs::copy(backup, target)?;
                }
            }
            None if target.exists() => {
                if target.is_dir() {
                    fs::remove_dir_all(target)?;
                } else {
                    fs::remove_file(target)?;
                }
            }
            None => {}
        }
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), HkError> {
    fs::create_dir_all(destination)?;
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(|error| HkError::Internal(error.to_string()))?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|error| HkError::Internal(error.to_string()))?;
        let target = destination.join(relative);
        if entry.file_type().is_symlink() {
            return Err(HkError::Validation(format!(
                "shared Skill tree contains a symlink: {}",
                entry.path().display()
            )));
        }
        if entry.file_type().is_dir() {
            fs::create_dir_all(target)?;
        } else if entry.file_type().is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn validate_targets(targets: &[PathBuf]) -> Result<(), HkError> {
    for path in targets {
        if !path.exists() {
            continue;
        }
        let raw = fs::read_to_string(path)?;
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => {
                serde_json::from_str::<serde_json::Value>(&raw)?;
            }
            Some("toml") => {
                raw.parse::<toml::Table>()?;
            }
            Some("yaml" | "yml") => {
                serde_yaml::from_str::<serde_yaml::Value>(&raw)
                    .map_err(|error| HkError::ConfigCorrupted(error.to_string()))?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn append_audit(home: &Path, id: &str, event: &str, detail: &str) -> Result<(), HkError> {
    let path = home.join(".harnesskit/steward/audit.jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let row = serde_json::json!({
        "timestamp": Utc::now(),
        "proposal_id": id,
        "event": event,
        "detail": detail,
    });
    writeln!(file, "{row}")?;
    Ok(())
}

fn json_to_toml(value: &serde_json::Value) -> Result<toml::Value, HkError> {
    match value {
        serde_json::Value::String(value) => Ok(toml::Value::String(value.clone())),
        serde_json::Value::Bool(value) => Ok(toml::Value::Boolean(*value)),
        serde_json::Value::Number(value) if value.is_i64() => {
            Ok(toml::Value::Integer(value.as_i64().unwrap_or_default()))
        }
        _ => Err(HkError::Validation(
            "Codex known config values must be strings, booleans, or integers".into(),
        )),
    }
}

fn json_to_yaml(value: &serde_json::Value) -> Result<serde_yaml::Value, HkError> {
    serde_yaml::to_value(value).map_err(|error| HkError::Validation(error.to_string()))
}

fn set_toml_path(root: &mut toml::Table, dotted: &str, value: toml::Value) -> Result<(), HkError> {
    let mut parts = dotted.split('.').peekable();
    let mut table = root;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            table.insert(part.into(), value);
            return Ok(());
        }
        let entry = table
            .entry(part)
            .or_insert_with(|| toml::Value::Table(Default::default()));
        table = entry
            .as_table_mut()
            .ok_or_else(|| HkError::ConfigCorrupted(format!("{part} is not a table")))?;
    }
    Err(HkError::Validation("empty config key".into()))
}

fn set_yaml_path(
    root: &mut serde_yaml::Value,
    dotted: &str,
    value: serde_yaml::Value,
) -> Result<(), HkError> {
    let parts = dotted.split('.').collect::<Vec<_>>();
    let mut current = root;
    for (index, part) in parts.iter().enumerate() {
        let mapping = current
            .as_mapping_mut()
            .ok_or_else(|| HkError::ConfigCorrupted(format!("{part} parent is not a mapping")))?;
        if index + 1 == parts.len() {
            mapping.insert((*part).into(), value);
            return Ok(());
        }
        current = mapping
            .entry((*part).into())
            .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));
    }
    Err(HkError::Validation("empty config key".into()))
}

fn set_json_path(
    root: &mut serde_json::Value,
    dotted: &str,
    value: serde_json::Value,
) -> Result<(), HkError> {
    let parts = dotted.split('.').collect::<Vec<_>>();
    let mut current = root;
    for (index, part) in parts.iter().enumerate() {
        let object = current
            .as_object_mut()
            .ok_or_else(|| HkError::ConfigCorrupted(format!("{part} parent is not an object")))?;
        if index + 1 == parts.len() {
            object.insert((*part).into(), value);
            return Ok(());
        }
        current = object.entry(*part).or_insert_with(|| serde_json::json!({}));
    }
    Err(HkError::Validation("empty config key".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_body_includes_provider_reasoning_controls() {
        let config: StewardConfig = serde_yaml::from_str(
            "base_url: https://api.deepseek.com/v1\nmodel: deepseek-v4-pro\napi_key_env: DEEPSEEK_API_KEY\nreasoning_effort: max\nthinking: true\n",
        )
        .unwrap();

        let body = build_chat_body(&config, "system".into(), "prompt").unwrap();

        assert_eq!(body["model"], "deepseek-v4-pro");
        assert_eq!(body["reasoning_effort"], "max");
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn free_chat_body_accepts_natural_language_and_includes_history() {
        let config: StewardConfig = serde_yaml::from_str(
            "base_url: https://api.deepseek.com/v1\nmodel: deepseek-v4-pro\napi_key_env: DEEPSEEK_API_KEY\nreasoning_effort: medium\n",
        )
        .unwrap();
        let history = vec![
            StewardChatMessage {
                role: StewardChatRole::User,
                content: "Which MCPs does Codex use?".into(),
            },
            StewardChatMessage {
                role: StewardChatRole::Steward,
                content: "Codex uses codegraph.".into(),
            },
        ];

        let body = build_free_chat_body(&config, "system".into(), &history, "Why?").unwrap();

        assert!(body.get("response_format").is_none());
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][2]["role"], "assistant");
        assert_eq!(body["messages"][3]["content"], "Why?");
        assert_eq!(body["reasoning_effort"], "medium");
    }

    #[test]
    fn natural_chat_reply_does_not_require_proposal_json() {
        let parsed = parse_chat_content("Codex currently has four enabled MCP servers.");

        assert_eq!(
            parsed.message,
            "Codex currently has four enabled MCP servers."
        );
        assert!(parsed.proposal.is_none());
        assert!(parsed.proposal_error.is_none());
    }

    #[test]
    fn optional_proposal_block_is_extracted_from_natural_reply() {
        let parsed = parse_chat_content(
            "I can make that change after approval.\n<harnesskit-proposal>\n{\"title\":\"Update Codex persona\",\"summary\":\"Make the response style concise\",\"risk\":\"low\",\"actions\":[{\"type\":\"persona_replace\",\"agent\":\"codex\",\"file\":\"AGENTS.md\",\"content\":\"Be concise.\"}]}\n</harnesskit-proposal>",
        );

        assert_eq!(parsed.message, "I can make that change after approval.");
        assert!(parsed.proposal.is_some());
        assert!(parsed.proposal_error.is_none());
    }

    #[test]
    fn malformed_optional_proposal_does_not_break_chat_reply() {
        let parsed = parse_chat_content(
            "Here is what I found.\n<harnesskit-proposal>{not-json}</harnesskit-proposal>",
        );

        assert_eq!(parsed.message, "Here is what I found.");
        assert!(parsed.proposal.is_none());
        assert!(parsed.proposal_error.is_some());
    }

    #[test]
    fn deterministic_mcp_prompt_becomes_pending_proposal() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".openclaw")).unwrap();
        fs::write(
            temp.path().join(".openclaw/openclaw.json"),
            r#"{"mcp":{"servers":{"docs":{"command":"docs-mcp"}}}}"#,
        )
        .unwrap();
        let proposal = propose(temp.path(), "enable MCP server \"docs\" for codex").unwrap();
        assert_eq!(proposal.status, "pending");
        assert!(proposal.diff.contains("docs"));
        assert_eq!(list_proposals(temp.path()).unwrap().len(), 1);
    }

    #[test]
    fn brain_questions_do_not_become_deterministic_proposals() {
        assert!(deterministic_model_proposal("How do shared skills work?").is_none());
        assert!(
            deterministic_model_proposal("Is MCP server data-agent enabled for codex?").is_none()
        );
    }

    #[test]
    fn rejects_memory_and_unknown_config_actions() {
        let model = ModelProposal {
            title: "bad".into(),
            summary: "bad".into(),
            risk: "low".into(),
            actions: vec![ModelAction::ConfigSet {
                agent: "codex".into(),
                key: "auth.token".into(),
                value: "secret".into(),
            }],
        };
        assert!(validate_model_proposal(Path::new("/tmp"), &model).is_err());
        assert!(persona_path(Path::new("/tmp"), "openclaw", "MEMORY.md").is_err());
    }

    #[test]
    fn external_model_schema_rejects_memory_actions() {
        let raw = r#"{
            "title":"bad",
            "summary":"bad",
            "risk":"low",
            "actions":[{
                "type":"memory_replace",
                "agent":"codex",
                "path":"/tmp/MEMORY.md",
                "content":"private"
            }]
        }"#;
        assert!(serde_json::from_str::<ModelProposal>(raw).is_err());
    }

    #[test]
    fn memory_paths_are_scoped_to_each_agent() {
        let temp = tempfile::tempdir().unwrap();
        let cases = [
            ("codex", temp.path().join(".codex/memories/codex.md")),
            ("hermes", temp.path().join(".hermes/memories/hermes.md")),
            (
                "openclaw",
                temp.path().join(".openclaw/workspace/memory/openclaw.md"),
            ),
        ];
        for (_, path) in &cases {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "old").unwrap();
        }

        for (agent, path) in &cases {
            assert_eq!(
                memory_path(temp.path(), agent, path.to_str().unwrap()).unwrap(),
                path.clone()
            );
        }

        assert!(memory_path(temp.path(), "codex", cases[1].1.to_str().unwrap()).is_err());
        let traversal = format!(
            "{}/../config.toml",
            temp.path().join(".codex/memories").display()
        );
        assert!(memory_path(temp.path(), "codex", &traversal).is_err());
        assert!(memory_path(temp.path(), "unknown", cases[0].1.to_str().unwrap()).is_err());
    }

    #[test]
    fn openclaw_memory_rejects_another_agents_private_directory() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".openclaw")).unwrap();
        let workspace = temp.path().join(".hermes/memories");
        fs::write(
            temp.path().join(".openclaw/openclaw.json"),
            serde_json::json!({
                "agents": {"defaults": {"workspace": workspace}}
            })
            .to_string(),
        )
        .unwrap();

        assert!(memory_path(
            temp.path(),
            "openclaw",
            temp.path()
                .join(".hermes/memories/MEMORY.md")
                .to_str()
                .unwrap(),
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn memory_path_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        let memory = temp.path().join(".openclaw/workspace/memory");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::write(outside.join("private.md"), "old").unwrap();
        symlink(&outside, memory.join("escape")).unwrap();

        assert!(memory_path(
            temp.path(),
            "openclaw",
            memory.join("escape/private.md").to_str().unwrap()
        )
        .is_err());
    }

    #[test]
    fn approved_memory_edit_uses_steward_write_flow() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".hermes/memories/MEMORY.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "old memory").unwrap();

        let proposal =
            propose_memory_edit(temp.path(), "hermes", path.to_str().unwrap(), "new memory")
                .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "old memory");

        let approved = approve(temp.path(), &proposal.id).unwrap();
        assert_eq!(approved.status, "approved");
        assert_eq!(fs::read_to_string(&path).unwrap(), "new memory");
        assert!(temp
            .path()
            .join(".harnesskit/backups")
            .join(&proposal.id)
            .exists());
        let audit =
            fs::read_to_string(temp.path().join(".harnesskit/steward/audit.jsonl")).unwrap();
        assert!(audit.contains("\"event\":\"proposed\""));
        assert!(audit.contains("\"event\":\"approved\""));
    }

    #[test]
    fn rejected_proposal_is_recorded_without_writing_target() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".hermes/memories/MEMORY.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "old memory").unwrap();

        let proposal =
            propose_memory_edit(temp.path(), "hermes", path.to_str().unwrap(), "new memory")
                .unwrap();
        let rejected = reject(temp.path(), &proposal.id).unwrap();

        assert_eq!(rejected.status, "rejected");
        assert_eq!(fs::read_to_string(&path).unwrap(), "old memory");
        assert!(!temp
            .path()
            .join(".harnesskit/backups")
            .join(&proposal.id)
            .exists());
        assert_eq!(list_proposals(temp.path()).unwrap()[0].status, "rejected");
        assert!(matches!(
            approve(temp.path(), &proposal.id),
            Err(HkError::Conflict(_))
        ));
        let audit =
            fs::read_to_string(temp.path().join(".harnesskit/steward/audit.jsonl")).unwrap();
        assert!(audit.contains("\"event\":\"rejected\""));
    }

    #[test]
    fn memory_edit_rejects_oversized_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".codex/memories/MEMORY.md");
        assert!(propose_memory_edit(
            temp.path(),
            "codex",
            path.to_str().unwrap(),
            &"x".repeat(256 * 1024 + 1),
        )
        .is_err());
    }

    #[test]
    fn model_context_omits_memory_content() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".hermes/memories")).unwrap();
        fs::write(
            temp.path().join(".hermes/memories/MEMORY.md"),
            "PRIVATE-MEMORY-MARKER",
        )
        .unwrap();

        let snapshot = brain::snapshot(temp.path()).unwrap();
        assert!(snapshot.agents[1].memory[0]
            .content
            .as_deref()
            .unwrap_or_default()
            .contains("PRIVATE-MEMORY-MARKER"));

        let context = safe_model_context(snapshot).to_string();
        assert!(!context.contains("PRIVATE-MEMORY-MARKER"));
        assert!(context.contains("\"memory_omitted\":true"));
    }

    #[test]
    fn hash_precondition_detects_external_edit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(&path, "model = \"a\"\n").unwrap();
        let hashes = BTreeMap::from([(
            path.to_string_lossy().into_owned(),
            hash_path(&path).unwrap(),
        )]);
        fs::write(&path, "model = \"b\"\n").unwrap();
        assert!(matches!(verify_hashes(&hashes), Err(HkError::Conflict(_))));
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(&path, "model = \"a\"\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        atomic_write(&path, b"model = \"b\"\n").unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn approval_updates_registry_and_native_openclaw_flag() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".openclaw")).unwrap();
        fs::write(
            temp.path().join(".openclaw/openclaw.json"),
            r#"{"mcp":{"servers":{"docs":{"command":"docs-mcp","enabled":true}}}}"#,
        )
        .unwrap();

        let proposal = propose(temp.path(), "disable MCP server \"docs\" for openclaw").unwrap();
        let approved = approve(temp.path(), &proposal.id).unwrap();
        assert_eq!(approved.status, "approved");

        let native: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(temp.path().join(".openclaw/openclaw.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(native["mcp"]["servers"]["docs"]["enabled"], false);

        let registry = crate::shared::load_mcp_registry(
            &temp.path().join(".harnesskit/shared/mcp-registry.yaml"),
        )
        .unwrap();
        assert!(!registry.agents["openclaw"].enabled["docs"]);
        assert!(temp
            .path()
            .join(".harnesskit/backups")
            .join(proposal.id)
            .exists());
    }
}
