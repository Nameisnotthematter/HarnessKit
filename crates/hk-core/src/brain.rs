use crate::adapter::{self, AgentAdapter, McpTransport};
use crate::shared::{self, SkillPlanDecision};
use crate::HkError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const BRAIN_AGENTS: &[&str] = &["codex", "hermes", "openclaw"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainFile {
    pub path: String,
    pub label: String,
    pub summary: String,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainAgent {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub status: String,
    pub config: Vec<BrainFile>,
    pub persona: Vec<BrainFile>,
    pub memory: Vec<BrainFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedSkillView {
    pub name: String,
    pub description: String,
    pub source: String,
    pub agents: Vec<String>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpRegistryView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub transport: String,
    pub agents: BTreeMap<String, bool>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainSnapshot {
    pub agents: Vec<BrainAgent>,
    pub shared_skills: Vec<SharedSkillView>,
    pub mcp_registry: Vec<McpRegistryView>,
    pub proposals: Vec<crate::steward::StewardProposal>,
    pub captured_at: DateTime<Utc>,
}

pub fn snapshot(home: &Path) -> Result<BrainSnapshot, HkError> {
    let adapters = adapters_for_home(home);
    let agents = adapters
        .iter()
        .map(|a| snapshot_agent(a.as_ref()))
        .collect();
    let shared_skills = snapshot_skills(home)?;
    let mcp_registry = snapshot_mcp(home, &adapters)?;
    let proposals = crate::steward::list_proposals(home)?;
    Ok(BrainSnapshot {
        agents,
        shared_skills,
        mcp_registry,
        proposals,
        captured_at: Utc::now(),
    })
}

pub fn adapters_for_home(home: &Path) -> Vec<Box<dyn AgentAdapter>> {
    vec![
        Box::new(adapter::codex::CodexAdapter::with_home_for_brain(
            home.to_path_buf(),
        )),
        Box::new(adapter::hermes::HermesAdapter::with_home_for_brain(
            home.to_path_buf(),
        )),
        Box::new(adapter::openclaw::OpenClawAdapter::with_home_for_brain(
            home.to_path_buf(),
        )),
    ]
}

fn snapshot_agent(adapter: &dyn AgentAdapter) -> BrainAgent {
    let settings = adapter.global_settings_files();
    let config = settings
        .iter()
        .map(|path| {
            brain_file(
                path,
                false,
                config_summary(adapter.name(), path),
                redacted_config_content(adapter.name(), path),
            )
        })
        .collect();
    let persona = adapter
        .global_rules_files()
        .iter()
        .map(|path| brain_file(path, false, text_summary(path), read_text_preview(path)))
        .collect();
    let memory = adapter
        .global_memory_files()
        .iter()
        .filter(|path| path.exists())
        .map(|path| {
            let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
            let within_limit = bytes <= crate::steward::MAX_MEMORY_EDIT_BYTES as u64;
            let full_content = within_limit
                .then(|| fs::read_to_string(path).ok())
                .flatten();
            let editable = full_content.is_some();
            brain_file(
                path,
                !editable,
                if editable {
                    format!("{bytes} bytes · private, editable with approval")
                } else if !within_limit {
                    format!(
                        "{bytes} bytes · private, read-only (over {} KiB limit)",
                        crate::steward::MAX_MEMORY_EDIT_BYTES / 1024
                    )
                } else {
                    format!("{bytes} bytes · private, read-only (not valid UTF-8)")
                },
                if within_limit {
                    full_content
                } else {
                    read_text_preview(path)
                },
            )
        })
        .collect();
    BrainAgent {
        id: adapter.name().into(),
        name: display_name(adapter.name()).into(),
        version: None,
        status: if adapter.detect() { "ready" } else { "offline" }.into(),
        config,
        persona,
        memory,
    }
}

fn brain_file(path: &Path, read_only: bool, summary: String, content: Option<String>) -> BrainFile {
    BrainFile {
        path: path.to_string_lossy().into_owned(),
        label: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        summary,
        exists: path.exists(),
        content,
        read_only,
    }
}

fn read_text_preview(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    Some(content.chars().take(65_536).collect())
}

fn redacted_config_content(agent: &str, path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let mut value = match agent {
        "codex" => serde_json::to_value(raw.parse::<toml::Table>().ok()?).ok()?,
        "hermes" => {
            let yaml = serde_yaml::from_str::<serde_yaml::Value>(&raw).ok()?;
            serde_json::to_value(yaml).ok()?
        }
        "openclaw" => serde_json::from_str::<serde_json::Value>(&raw).ok()?,
        _ => return None,
    };
    redact_sensitive_values(&mut value, None);
    let rendered = serde_json::to_string_pretty(&value).ok()?;
    Some(rendered.chars().take(65_536).collect())
}

fn redact_sensitive_values(value: &mut serde_json::Value, parent_key: Option<&str>) {
    let parent_sensitive = parent_key.is_some_and(is_sensitive_key);
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                if parent_sensitive || is_sensitive_key(key) {
                    if let serde_json::Value::Object(keys) = child {
                        for value in keys.values_mut() {
                            *value = serde_json::Value::String("<redacted>".into());
                        }
                    } else {
                        *child = serde_json::Value::String("<redacted>".into());
                    }
                } else {
                    redact_sensitive_values(child, Some(key));
                }
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                redact_sensitive_values(child, parent_key);
            }
        }
        _ if parent_sensitive => *value = serde_json::Value::String("<redacted>".into()),
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "credential",
        "authorization",
        "api_key",
        "apikey",
        "env",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn text_summary(path: &Path) -> String {
    let Ok(content) = fs::read_to_string(path) else {
        return "Not created".into();
    };
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("<!--"))
        .unwrap_or("Empty file")
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(180)
        .collect()
}

fn config_summary(agent: &str, path: &Path) -> String {
    let Ok(content) = fs::read_to_string(path) else {
        return "Not created".into();
    };
    let fields: Vec<(&str, Option<String>)> = match agent {
        "codex" => content
            .parse::<toml::Table>()
            .ok()
            .map(|doc| {
                [
                    "model",
                    "model_reasoning_effort",
                    "personality",
                    "sandbox_mode",
                ]
                .into_iter()
                .map(|key| (key, doc.get(key).and_then(safe_toml_scalar)))
                .collect()
            })
            .unwrap_or_default(),
        "hermes" => serde_yaml::from_str::<serde_yaml::Value>(&content)
            .ok()
            .map(|doc| {
                ["model", "provider", "display.personality"]
                    .into_iter()
                    .map(|key| (key, yaml_pointer(&doc, key)))
                    .collect()
            })
            .unwrap_or_default(),
        "openclaw" => serde_json::from_str::<serde_json::Value>(&content)
            .ok()
            .map(|doc| {
                ["/agents/defaults/model", "/agents/defaults/workspace"]
                    .into_iter()
                    .map(|key| {
                        (
                            key.trim_start_matches('/'),
                            doc.pointer(key).and_then(safe_json_scalar),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => vec![],
    };
    let summary = fields
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| format!("{key}={value}")))
        .collect::<Vec<_>>()
        .join(" · ");
    if summary.is_empty() {
        "Structured config; sensitive fields hidden".into()
    } else {
        summary
    }
}

fn safe_toml_scalar(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(value) => Some(value.clone()),
        toml::Value::Boolean(value) => Some(value.to_string()),
        toml::Value::Integer(value) => Some(value.to_string()),
        _ => None,
    }
}

fn safe_json_scalar(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn yaml_pointer(root: &serde_yaml::Value, dotted: &str) -> Option<String> {
    let mut value = root;
    for part in dotted.split('.') {
        value = value.get(part)?;
    }
    value
        .as_str()
        .map(String::from)
        .or_else(|| value.as_bool().map(|v| v.to_string()))
        .or_else(|| value.as_i64().map(|v| v.to_string()))
}

fn snapshot_skills(home: &Path) -> Result<Vec<SharedSkillView>, HkError> {
    let canonical = home.join(".agents/skills");
    let sources = [
        shared::SkillSource {
            agent: "codex".into(),
            root: home.join(".codex/skills"),
        },
        shared::SkillSource {
            agent: "hermes".into(),
            root: home.join(".hermes/skills/local"),
        },
        shared::SkillSource {
            agent: "openclaw".into(),
            root: home.join(".openclaw/skills"),
        },
    ];
    let plan = shared::plan_skill_migration(&canonical, &sources)
        .map_err(|error| HkError::Internal(error.to_string()))?;
    let mut names = BTreeSet::new();
    let mut result = Vec::new();
    for skill in shared::inventory_skills(&canonical)
        .map_err(|error| HkError::Internal(error.to_string()))?
    {
        names.insert(skill.name.clone());
        result.push(SharedSkillView {
            description: skill_description(&skill.path),
            source: skill.path.to_string_lossy().into_owned(),
            name: skill.name,
            agents: BRAIN_AGENTS.iter().map(|agent| (*agent).into()).collect(),
            status: "ready".into(),
        });
    }
    for item in plan.items {
        if names.contains(&item.name) {
            continue;
        }
        let source = item
            .sources
            .first()
            .map(|origin| origin.path.clone())
            .unwrap_or(item.destination);
        result.push(SharedSkillView {
            description: skill_description(&source),
            source: source.to_string_lossy().into_owned(),
            name: item.name,
            agents: item
                .sources
                .into_iter()
                .map(|origin| origin.agent)
                .collect(),
            status: if item.decision == SkillPlanDecision::Conflict {
                "unavailable"
            } else {
                "needs_setup"
            }
            .into(),
        });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

fn skill_description(path: &Path) -> String {
    text_summary(&path.join("SKILL.md"))
}

fn snapshot_mcp(
    home: &Path,
    adapters: &[Box<dyn AgentAdapter>],
) -> Result<Vec<McpRegistryView>, HkError> {
    let mut discovered: BTreeMap<String, (McpTransport, BTreeMap<String, bool>)> = BTreeMap::new();
    let mut native_agents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for adapter in adapters {
        for server in adapter.read_mcp_servers() {
            native_agents
                .entry(server.name.clone())
                .or_default()
                .insert(adapter.name().into());
            let row = discovered
                .entry(server.name)
                .or_insert_with(|| (server.transport, default_agent_flags()));
            row.1.insert(adapter.name().into(), server.enabled);
        }
    }

    let registry_path = home.join(".harnesskit/shared/mcp-registry.yaml");
    let mut managed = BTreeSet::new();
    if registry_path.exists() {
        let registry = shared::load_mcp_registry(&registry_path)
            .map_err(|error| HkError::ConfigCorrupted(error.to_string()))?;
        for (name, server) in &registry.servers {
            managed.insert(name.clone());
            let transport = if server.url.is_some() {
                McpTransport::Http
            } else {
                McpTransport::Stdio
            };
            let row = discovered
                .entry(name.clone())
                .or_insert_with(|| (transport, default_agent_flags()));
            row.0 = transport;
            for agent in BRAIN_AGENTS {
                if let Some(enabled) = registry
                    .agents
                    .get(*agent)
                    .and_then(|config| config.enabled.get(name))
                {
                    row.1.insert((*agent).into(), *enabled);
                } else if !native_agents
                    .get(name)
                    .is_some_and(|agents| agents.contains(*agent))
                {
                    row.1.insert((*agent).into(), server.enabled_by_default);
                }
            }
        }
    }

    Ok(discovered
        .into_iter()
        .map(|(name, (transport, agents))| McpRegistryView {
            id: name.clone(),
            description: if managed.contains(&name) {
                "Managed by the central secret-free registry"
            } else {
                "Discovered in native config; adopt before cross-agent deployment"
            }
            .into(),
            name,
            transport: match transport {
                McpTransport::Stdio => "stdio",
                McpTransport::Http => "http",
                McpTransport::Sse => "sse",
            }
            .into(),
            status: if agents.values().any(|enabled| *enabled) {
                "configured"
            } else {
                "disabled"
            }
            .into(),
            agents,
        })
        .collect())
}

fn default_agent_flags() -> BTreeMap<String, bool> {
    BRAIN_AGENTS
        .iter()
        .map(|agent| ((*agent).into(), false))
        .collect()
}

fn display_name(agent: &str) -> &str {
    match agent {
        "codex" => "Codex",
        "hermes" => "Hermes",
        "openclaw" => "OpenClaw",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_preview_redacts_nested_secrets() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("openclaw.json");
        fs::write(
            &path,
            r#"{"mcp":{"servers":{"private":{"env":{"API_KEY":"do-not-show"},"headers":{"Authorization":"Bearer do-not-show"}}}}}"#,
        )
        .unwrap();

        let preview = redacted_config_content("openclaw", &path).unwrap();

        assert!(!preview.contains("do-not-show"));
        assert!(preview.contains("<redacted>"));
    }

    #[test]
    fn native_mcp_status_describes_configuration_not_health() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".openclaw")).unwrap();
        fs::write(
            temp.path().join(".openclaw/openclaw.json"),
            r#"{"mcp":{"servers":{"docs":{"command":"docs-mcp","enabled":true}}}}"#,
        )
        .unwrap();

        let adapters = adapters_for_home(temp.path());
        let entries = snapshot_mcp(temp.path(), &adapters).unwrap();

        assert_eq!(entries[0].status, "configured");
    }

    #[test]
    fn shared_registry_merges_with_native_mcp_discovery() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".openclaw")).unwrap();
        fs::write(
            temp.path().join(".openclaw/openclaw.json"),
            r#"{"mcp":{"servers":{"native-only":{"command":"native-mcp","enabled":true},"shared":{"command":"shared-mcp","enabled":true}}}}"#,
        )
        .unwrap();
        fs::create_dir_all(temp.path().join(".harnesskit/shared")).unwrap();
        fs::write(
            temp.path().join(".harnesskit/shared/mcp-registry.yaml"),
            r#"version: 1
servers:
  registry-only:
    command: registry-mcp
  shared:
    command: shared-mcp
    enabled_by_default: false
agents:
  codex:
    enabled:
      registry-only: true
      shared: false
"#,
        )
        .unwrap();

        let adapters = adapters_for_home(temp.path());
        let entries = snapshot_mcp(temp.path(), &adapters).unwrap();

        let names = entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["native-only", "registry-only", "shared"]);
        let native = entries
            .iter()
            .find(|entry| entry.name == "native-only")
            .unwrap();
        assert!(native.agents["openclaw"]);
        let registry = entries
            .iter()
            .find(|entry| entry.name == "registry-only")
            .unwrap();
        assert!(registry.agents["codex"]);
        let shared = entries
            .iter()
            .find(|entry| entry.name == "shared")
            .unwrap();
        assert!(!shared.agents["codex"]);
        assert!(shared.agents["openclaw"]);
    }

    #[test]
    fn memory_snapshot_is_complete_within_edit_limit() {
        let temp = tempfile::tempdir().unwrap();
        let memory = temp.path().join(".codex/memories/MEMORY.md");
        fs::create_dir_all(memory.parent().unwrap()).unwrap();
        let content = "x".repeat(70 * 1024);
        fs::write(&memory, &content).unwrap();

        let snapshot = snapshot(temp.path()).unwrap();
        let file = &snapshot.agents[0].memory[0];

        assert_eq!(file.content.as_deref(), Some(content.as_str()));
        assert!(!file.read_only);
    }

    #[test]
    fn oversized_memory_snapshot_is_read_only() {
        let temp = tempfile::tempdir().unwrap();
        let memory = temp.path().join(".codex/memories/MEMORY.md");
        fs::create_dir_all(memory.parent().unwrap()).unwrap();
        fs::write(
            &memory,
            vec![b'x'; crate::steward::MAX_MEMORY_EDIT_BYTES + 1],
        )
        .unwrap();

        let snapshot = snapshot(temp.path()).unwrap();
        let file = &snapshot.agents[0].memory[0];

        assert!(file.read_only);
        assert!(file.summary.contains("over 256 KiB limit"));
    }

    #[test]
    fn non_utf8_memory_snapshot_is_read_only() {
        let temp = tempfile::tempdir().unwrap();
        let memory = temp.path().join(".codex/memories/MEMORY.md");
        fs::create_dir_all(memory.parent().unwrap()).unwrap();
        fs::write(&memory, [0xff, 0xfe]).unwrap();

        let snapshot = snapshot(temp.path()).unwrap();
        let file = &snapshot.agents[0].memory[0];

        assert!(file.read_only);
        assert!(file.summary.contains("not valid UTF-8"));
    }
}
