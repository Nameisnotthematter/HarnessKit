// OpenClaw 2026.7.1 configuration surfaces:
// - root:      ~/.openclaw
// - config:    ~/.openclaw/openclaw.json
// - skills:    ~/.agents/skills, ~/.openclaw/skills, <workspace>/skills
// - MCP:       openclaw.json at mcp.servers.<name>
// - persona:   <workspace>/{AGENTS,SOUL,IDENTITY,USER,TOOLS,HEARTBEAT,DREAMS}.md
// - memory:    <workspace>/MEMORY.md and <workspace>/memory/**/*.md

use super::{
    AgentAdapter, HookEntry, HookFormat, McpFormat, McpServerEntry, McpTransport, PluginEntry,
    ProjectMarker, RemoteMcpSchema,
};
use std::path::{Path, PathBuf};

const PERSONA_FILES: &[&str] = &[
    "AGENTS.md",
    "SOUL.md",
    "IDENTITY.md",
    "USER.md",
    "TOOLS.md",
    "HEARTBEAT.md",
    "DREAMS.md",
];

pub struct OpenClawAdapter {
    home: PathBuf,
}

impl Default for OpenClawAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenClawAdapter {
    pub fn new() -> Self {
        Self {
            home: dirs::home_dir().unwrap_or_default(),
        }
    }

    #[cfg(test)]
    pub fn with_home(home: PathBuf) -> Self {
        Self { home }
    }

    pub(crate) fn with_home_for_brain(home: PathBuf) -> Self {
        Self { home }
    }

    fn parse_json(path: &Path) -> Option<serde_json::Value> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// OpenClaw allows the default workspace to move. Resolve that known field
    /// without interpreting any other config content; malformed or unsupported
    /// paths fall back to the conventional ~/.openclaw/workspace directory.
    fn workspace_dir(&self) -> PathBuf {
        let fallback = self.base_dir().join("workspace");
        let Some(raw) = Self::parse_json(&self.mcp_config_path()).and_then(|config| {
            config
                .pointer("/agents/defaults/workspace")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        }) else {
            return fallback;
        };

        if let Some(relative) = raw.strip_prefix("~/") {
            self.home.join(relative)
        } else {
            let path = PathBuf::from(raw);
            if path.is_absolute() {
                path
            } else {
                fallback
            }
        }
    }

    fn parse_mcp_entry(name: &str, value: &serde_json::Value) -> Option<McpServerEntry> {
        let transport_name = value.get("transport").and_then(|v| v.as_str());
        let url = value.get("url").and_then(|v| v.as_str()).map(String::from);
        let command = value
            .get("command")
            .and_then(|v| v.as_str())
            .map(String::from);

        let transport = match transport_name {
            Some("stdio") => McpTransport::Stdio,
            Some("streamable-http") => McpTransport::Http,
            Some("sse") => McpTransport::Sse,
            Some(_) => return None,
            None if url.is_some() => McpTransport::Http,
            None => McpTransport::Stdio,
        };

        if transport == McpTransport::Stdio && command.is_none() {
            return None;
        }
        if transport != McpTransport::Stdio && url.is_none() {
            return None;
        }

        Some(McpServerEntry {
            name: name.to_string(),
            command: if transport == McpTransport::Stdio {
                command.unwrap_or_default()
            } else {
                String::new()
            },
            args: super::json_string_vec(value, "args"),
            env: super::json_string_map(value, "env"),
            transport,
            url,
            headers: super::json_string_map(value, "headers"),
            enabled: value
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
        })
    }
}

impl AgentAdapter for OpenClawAdapter {
    fn name(&self) -> &str {
        "openclaw"
    }

    fn base_dir(&self) -> PathBuf {
        self.home.join(".openclaw")
    }

    fn detect(&self) -> bool {
        self.base_dir().exists()
    }

    fn skill_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.home.join(".agents").join("skills"),
            self.base_dir().join("skills"),
            self.workspace_dir().join("skills"),
        ]
    }

    fn mcp_config_path(&self) -> PathBuf {
        self.base_dir().join("openclaw.json")
    }

    fn mcp_format(&self) -> McpFormat {
        McpFormat::OpenClaw
    }

    fn remote_mcp_schema(&self) -> RemoteMcpSchema {
        RemoteMcpSchema::OpenClaw
    }

    fn supports_native_mcp_toggle(&self) -> bool {
        true
    }

    fn read_mcp_servers(&self) -> Vec<McpServerEntry> {
        self.read_mcp_servers_from(&self.mcp_config_path())
    }

    fn read_mcp_servers_from(&self, path: &Path) -> Vec<McpServerEntry> {
        let Some(config) = Self::parse_json(path) else {
            return vec![];
        };
        let Some(servers) = config
            .get("mcp")
            .and_then(|v| v.get("servers"))
            .and_then(|v| v.as_object())
        else {
            return vec![];
        };

        servers
            .iter()
            .filter_map(|(name, value)| Self::parse_mcp_entry(name, value))
            .collect()
    }

    fn hook_config_path(&self) -> PathBuf {
        self.mcp_config_path()
    }

    fn hook_format(&self) -> HookFormat {
        HookFormat::None
    }

    fn read_hooks(&self) -> Vec<HookEntry> {
        vec![]
    }

    fn plugin_dirs(&self) -> Vec<PathBuf> {
        vec![self.base_dir().join("extensions")]
    }

    fn read_plugins(&self) -> Vec<PluginEntry> {
        vec![]
    }

    fn global_rules_files(&self) -> Vec<PathBuf> {
        let workspace = self.workspace_dir();
        PERSONA_FILES
            .iter()
            .map(|name| workspace.join(name))
            .collect()
    }

    fn global_memory_files(&self) -> Vec<PathBuf> {
        let workspace = self.workspace_dir();
        let mut files = vec![workspace.join("MEMORY.md")];
        files.extend(super::files_with_ext_recursive(
            &workspace.join("memory"),
            "md",
        ));
        files.sort();
        files.dedup();
        files
    }

    fn global_settings_files(&self) -> Vec<PathBuf> {
        vec![self.mcp_config_path()]
    }

    fn project_markers(&self) -> Vec<ProjectMarker> {
        vec![ProjectMarker::File("SOUL.md")]
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AgentAdapter, McpTransport};
    use super::*;

    #[test]
    fn detects_openclaw_root() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = OpenClawAdapter::with_home(tmp.path().to_path_buf());
        assert!(!adapter.detect());

        std::fs::create_dir_all(tmp.path().join(".openclaw")).unwrap();
        assert!(adapter.detect());
    }

    #[test]
    fn discovers_shared_native_and_workspace_skills_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = OpenClawAdapter::with_home(tmp.path().to_path_buf());
        assert_eq!(
            adapter.skill_dirs(),
            vec![
                tmp.path().join(".agents/skills"),
                tmp.path().join(".openclaw/skills"),
                tmp.path().join(".openclaw/workspace/skills"),
            ]
        );
    }

    #[test]
    fn uses_openclaw_soul_file_as_workspace_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = OpenClawAdapter::with_home(tmp.path().to_path_buf());
        assert_eq!(
            adapter.project_markers(),
            vec![ProjectMarker::File("SOUL.md")]
        );
    }

    #[test]
    fn configured_workspace_drives_brain_file_discovery() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(".openclaw");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("openclaw.json"),
            r#"{"agents":{"defaults":{"workspace":"~/custom-brain"}}}"#,
        )
        .unwrap();

        let adapter = OpenClawAdapter::with_home(tmp.path().to_path_buf());
        assert_eq!(
            adapter.global_rules_files()[0],
            tmp.path().join("custom-brain/AGENTS.md")
        );
        assert_eq!(
            adapter.skill_dirs()[2],
            tmp.path().join("custom-brain/skills")
        );
    }

    #[test]
    fn reads_nested_mcp_servers_and_transports() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(".openclaw");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("openclaw.json"),
            r#"{
              "mcp": {"servers": {
                "local": {
                  "command": "npx",
                  "args": ["-y", "server"],
                  "env": {"TOKEN": "secret", "IGNORED_NUMBER": 1},
                  "enabled": false
                },
                "http": {
                  "url": "https://example.com/mcp",
                  "transport": "streamable-http",
                  "headers": {"Authorization": "Bearer secret"}
                },
                "events": {
                  "url": "https://example.com/events",
                  "transport": "sse"
                },
                "invalid": {"transport": "stdio"}
              }}
            }"#,
        )
        .unwrap();

        let adapter = OpenClawAdapter::with_home(tmp.path().to_path_buf());
        let servers = adapter.read_mcp_servers();
        assert_eq!(servers.len(), 3, "malformed entries must be skipped");

        let local = servers.iter().find(|s| s.name == "local").unwrap();
        assert_eq!(local.transport, McpTransport::Stdio);
        assert_eq!(local.command, "npx");
        assert_eq!(local.args, vec!["-y", "server"]);
        assert!(!local.enabled);
        assert_eq!(local.env.get("TOKEN").map(String::as_str), Some("secret"));
        assert!(!local.env.contains_key("IGNORED_NUMBER"));

        let http = servers.iter().find(|s| s.name == "http").unwrap();
        assert_eq!(http.transport, McpTransport::Http);
        assert_eq!(http.url.as_deref(), Some("https://example.com/mcp"));
        assert_eq!(http.command, "");

        let events = servers.iter().find(|s| s.name == "events").unwrap();
        assert_eq!(events.transport, McpTransport::Sse);
        assert!(events.enabled, "missing enabled defaults to true");
    }

    #[test]
    fn discovers_main_and_recursive_memory_files() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join(".openclaw/workspace");
        std::fs::create_dir_all(workspace.join("memory/archive")).unwrap();
        std::fs::write(workspace.join("MEMORY.md"), "main").unwrap();
        std::fs::write(workspace.join("memory/today.md"), "today").unwrap();
        std::fs::write(workspace.join("memory/archive/old.md"), "old").unwrap();
        std::fs::write(workspace.join("memory/ignore.txt"), "ignore").unwrap();

        let adapter = OpenClawAdapter::with_home(tmp.path().to_path_buf());
        let files = adapter.global_memory_files();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&workspace.join("MEMORY.md")));
        assert!(files.contains(&workspace.join("memory/today.md")));
        assert!(files.contains(&workspace.join("memory/archive/old.md")));
    }
}
