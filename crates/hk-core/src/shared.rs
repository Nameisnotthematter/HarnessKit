//! Portable shared skills and MCP registry models.
//!
//! This module only inventories state and produces plans. It never copies,
//! removes, or otherwise migrates skills.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REGISTRY_VERSION: u32 = 1;

pub fn canonical_skills_root() -> Result<PathBuf> {
    Ok(home_dir()?.join(".agents/skills"))
}

pub fn default_mcp_registry_path() -> Result<PathBuf> {
    Ok(home_dir()?.join(".harnesskit/shared/mcp-registry.yaml"))
}

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("home directory is unavailable")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillSource {
    pub agent: String,
    pub root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillInventoryEntry {
    pub name: String,
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillOrigin {
    pub agent: String,
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillPlanDecision {
    Add,
    AlreadyCanonical,
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillPlanItem {
    pub name: String,
    pub destination: PathBuf,
    pub canonical_sha256: Option<String>,
    pub sources: Vec<SkillOrigin>,
    pub decision: SkillPlanDecision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillMigrationPlan {
    pub canonical_root: PathBuf,
    pub items: Vec<SkillPlanItem>,
}

/// Inventories immediate skill directories. Missing roots are treated as empty.
/// The reserved `.system` directory and directories without `SKILL.md` are skipped.
pub fn inventory_skills(root: &Path) -> Result<Vec<SkillInventoryEntry>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", root.display())),
    };

    let mut inventory = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("read entry in {}", root.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name == ".system" || !entry.file_type()?.is_dir() || !path.join("SKILL.md").is_file() {
            continue;
        }
        inventory.push(SkillInventoryEntry {
            name,
            sha256: hash_skill_directory(&path)?,
            path,
        });
    }
    inventory.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(inventory)
}

/// Produces a read-only migration plan. No filesystem writes are performed.
pub fn plan_skill_migration(
    canonical_root: &Path,
    sources: &[SkillSource],
) -> Result<SkillMigrationPlan> {
    let canonical: BTreeMap<_, _> = inventory_skills(canonical_root)?
        .into_iter()
        .map(|entry| (entry.name.clone(), entry))
        .collect();
    let mut grouped: BTreeMap<String, Vec<SkillOrigin>> = BTreeMap::new();

    for source in sources {
        for entry in inventory_skills(&source.root)? {
            grouped.entry(entry.name).or_default().push(SkillOrigin {
                agent: source.agent.clone(),
                path: entry.path,
                sha256: entry.sha256,
            });
        }
    }

    let mut items = Vec::new();
    for (name, mut origins) in grouped {
        origins.sort_by(|left, right| {
            left.agent
                .cmp(&right.agent)
                .then_with(|| left.path.cmp(&right.path))
        });
        let canonical_sha256 = canonical.get(&name).map(|entry| entry.sha256.clone());
        let hashes: BTreeSet<_> = origins
            .iter()
            .map(|origin| origin.sha256.as_str())
            .chain(canonical_sha256.iter().map(String::as_str))
            .collect();
        let decision = if hashes.len() > 1 {
            SkillPlanDecision::Conflict
        } else if canonical_sha256.is_some() {
            SkillPlanDecision::AlreadyCanonical
        } else {
            SkillPlanDecision::Add
        };
        items.push(SkillPlanItem {
            destination: canonical_root.join(&name),
            name,
            canonical_sha256,
            sources: origins,
            decision,
        });
    }

    Ok(SkillMigrationPlan {
        canonical_root: canonical_root.to_path_buf(),
        items,
    })
}

fn hash_skill_directory(root: &Path) -> Result<String> {
    fn collect(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(current)
            .with_context(|| format!("read skill directory {}", current.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                collect(root, &path, files)?;
            } else if file_type.is_file() || file_type.is_symlink() {
                files.push(path.strip_prefix(root)?.to_path_buf());
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for relative in files {
        let path = root.join(&relative);
        let relative = relative.to_string_lossy();
        hasher.update(relative.len().to_le_bytes());
        hasher.update(relative.as_bytes());
        let bytes = if path.symlink_metadata()?.file_type().is_symlink() {
            fs::read_link(&path)?.to_string_lossy().as_bytes().to_vec()
        } else {
            fs::read(&path).with_context(|| format!("read {}", path.display()))?
        };
        hasher.update(bytes.len().to_le_bytes());
        hasher.update(bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct McpRegistry {
    pub version: u32,
    #[serde(default)]
    pub servers: BTreeMap<String, McpServer>,
    #[serde(default)]
    pub agents: BTreeMap<String, AgentMcpConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct McpServer {
    #[serde(default = "default_true")]
    pub enabled_by_default: bool,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub env_keys: Vec<String>,
    #[serde(default)]
    pub env_secret_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub header_secret_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub tool_allow: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentMcpConfig {
    #[serde(default)]
    pub enabled: BTreeMap<String, bool>,
    #[serde(default)]
    pub overrides: BTreeMap<String, McpServerOverride>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct McpServerOverride {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub env_keys: Option<Vec<String>>,
    #[serde(default)]
    pub env_secret_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub header_secret_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub tool_allow: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentMcpPlan {
    pub agent: String,
    pub servers: BTreeMap<String, McpServer>,
}

pub fn load_mcp_registry(path: &Path) -> Result<McpRegistry> {
    let yaml = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let registry: McpRegistry =
        serde_yaml::from_str(&yaml).with_context(|| format!("parse {}", path.display()))?;
    registry.validate()?;
    Ok(registry)
}

impl McpRegistry {
    pub fn validate(&self) -> Result<()> {
        if self.version != REGISTRY_VERSION {
            bail!("unsupported MCP registry version {}", self.version);
        }
        for (name, server) in &self.servers {
            validate_server(name, server)?;
        }
        for (agent, config) in &self.agents {
            for server in config.enabled.keys().chain(config.overrides.keys()) {
                if !self.servers.contains_key(server) {
                    bail!("agent {agent} references unknown MCP server {server}");
                }
            }
            for (server, override_) in &config.overrides {
                let resolved = apply_override(&self.servers[server], override_)?;
                validate_server(server, &resolved)?;
            }
        }
        Ok(())
    }

    pub fn plan_for_agent(&self, agent: &str) -> Result<AgentMcpPlan> {
        self.validate()?;
        let config = self.agents.get(agent);
        let mut servers = BTreeMap::new();
        for (name, server) in &self.servers {
            let enabled = config
                .and_then(|config| config.enabled.get(name))
                .copied()
                .unwrap_or(server.enabled_by_default);
            if !enabled {
                continue;
            }
            let resolved = match config.and_then(|config| config.overrides.get(name)) {
                Some(override_) => apply_override(server, override_)?,
                None => server.clone(),
            };
            servers.insert(name.clone(), resolved);
        }
        Ok(AgentMcpPlan {
            agent: agent.to_owned(),
            servers,
        })
    }
}

fn apply_override(base: &McpServer, override_: &McpServerOverride) -> Result<McpServer> {
    if override_.command.is_some() && override_.url.is_some() {
        bail!("MCP override cannot set both command and url");
    }
    let mut resolved = base.clone();
    if let Some(command) = &override_.command {
        resolved.command = Some(command.clone());
        resolved.url = None;
    }
    if let Some(url) = &override_.url {
        resolved.url = Some(url.clone());
        resolved.command = None;
    }
    if let Some(transport) = &override_.transport {
        resolved.transport = Some(transport.clone());
    }
    if let Some(args) = &override_.args {
        resolved.args = args.clone();
    }
    if let Some(cwd) = &override_.cwd {
        resolved.cwd = Some(cwd.clone());
    }
    if let Some(env_keys) = &override_.env_keys {
        resolved.env_keys = env_keys.clone();
    }
    resolved
        .env_secret_refs
        .extend(override_.env_secret_refs.clone());
    resolved
        .header_secret_refs
        .extend(override_.header_secret_refs.clone());
    if let Some(tool_allow) = &override_.tool_allow {
        resolved.tool_allow = tool_allow.clone();
    }
    Ok(resolved)
}

fn validate_server(name: &str, server: &McpServer) -> Result<()> {
    if server.command.is_some() == server.url.is_some() {
        bail!("MCP server {name} must set exactly one of command or url");
    }
    match (server.url.is_some(), server.transport.as_deref()) {
        (false, None | Some("stdio")) => {}
        (true, None | Some("http") | Some("sse")) => {}
        (_, Some(other)) => bail!("MCP server {name} has unsupported transport {other}"),
    }
    for key in &server.env_keys {
        validate_env_key(key)?;
    }
    for (key, reference) in &server.env_secret_refs {
        validate_env_key(key)?;
        validate_secret_ref(reference)?;
    }
    for (header, reference) in &server.header_secret_refs {
        if header.trim().is_empty() {
            bail!("MCP server {name} has an empty header name");
        }
        validate_secret_ref(reference)?;
    }
    Ok(())
}

fn validate_env_key(key: &str) -> Result<()> {
    let mut chars = key.chars();
    if !matches!(chars.next(), Some('_' | 'A'..='Z'))
        || !chars.all(|character| matches!(character, '_' | 'A'..='Z' | '0'..='9'))
    {
        bail!("invalid environment key {key:?}");
    }
    Ok(())
}

fn validate_secret_ref(reference: &str) -> Result<()> {
    let key = reference
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
        .context("secret references must use ${ENV_NAME}")?;
    validate_env_key(key)
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, name: &str, body: &str) {
        let skill = root.join(name);
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn migration_plan_detects_hash_conflict_and_excludes_system() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("canonical");
        let source = temp.path().join("source");
        write_skill(&canonical, "shared", "canonical");
        write_skill(&source, "shared", "different");
        write_skill(&source, ".system", "ignored");

        let plan = plan_skill_migration(
            &canonical,
            &[SkillSource {
                agent: "codex".into(),
                root: source,
            }],
        )
        .unwrap();

        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].name, "shared");
        assert_eq!(plan.items[0].decision, SkillPlanDecision::Conflict);
    }

    #[test]
    fn registry_rejects_raw_secret_fields_and_values() {
        let raw_env = r#"
version: 1
servers:
  unsafe:
    command: tool
    env:
      API_TOKEN: plaintext
"#;
        assert!(serde_yaml::from_str::<McpRegistry>(raw_env).is_err());

        let raw_header = r#"
version: 1
servers:
  unsafe:
    url: https://example.test/mcp
    header_secret_refs:
      Authorization: Bearer plaintext
"#;
        let registry: McpRegistry = serde_yaml::from_str(raw_header).unwrap();
        assert!(registry.validate().is_err());
    }

    #[test]
    fn registry_plans_enabled_server_with_safe_override() {
        let yaml = r#"
version: 1
servers:
  docs:
    url: https://example.test/mcp
    enabled_by_default: false
    header_secret_refs:
      Authorization: ${DOCS_TOKEN}
agents:
  codex:
    enabled:
      docs: true
    overrides:
      docs:
        header_secret_refs:
          X-Api-Key: ${CODEX_DOCS_TOKEN}
"#;
        let registry: McpRegistry = serde_yaml::from_str(yaml).unwrap();
        let plan = registry.plan_for_agent("codex").unwrap();
        assert_eq!(plan.servers.len(), 1);
        assert_eq!(plan.servers["docs"].header_secret_refs.len(), 2);
    }
}
