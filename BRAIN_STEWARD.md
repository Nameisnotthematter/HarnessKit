# Brain Steward

HarnessKit's Brain page manages Codex, Hermes, and OpenClaw while keeping each
runtime's memory private. It shares only Skills and MCP definitions.

Configure an OpenAI-compatible backend at
`~/.harnesskit/steward/config.yaml`:

```yaml
base_url: https://api.openai.com/v1
model: gpt-5.4
api_key_env: OPENAI_API_KEY
```

The API key is read from the named environment variable and never persisted by
HarnessKit. Memory content is omitted from model requests.

Run the independent, proposal-only service with:

```sh
hk steward --port 7071
```

It binds to localhost and exposes only snapshot and proposal endpoints. It has
no approval endpoint. Brain-file writes can only be approved in the HarnessKit
UI, which rechecks source hashes, creates backups, writes atomically, validates,
and rolls back on failure.

The secret-free MCP registry lives at
`~/.harnesskit/shared/mcp-registry.yaml`; shared Skills live at
`~/.agents/skills`.
