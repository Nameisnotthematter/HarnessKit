import { Network } from "lucide-react";
import type { BrainAgentId, McpRegistryEntry } from "@/stores/brain-store";

const agents: BrainAgentId[] = ["codex", "hermes", "openclaw"];

interface McpRegistryProps {
  entries: McpRegistryEntry[];
  proposing: boolean;
  onRequestToggle: (entry: McpRegistryEntry, agent: BrainAgentId) => void;
}

export function McpRegistry({
  entries,
  proposing,
  onRequestToggle,
}: McpRegistryProps) {
  return (
    <section className="rounded-xl border border-border bg-card p-4 shadow-sm">
      <div className="flex items-center gap-2">
        <Network size={16} className="text-primary" />
        <h2 className="font-semibold">MCP Registry</h2>
      </div>
      <p className="mt-1 text-xs text-muted-foreground">
        Per-agent switches create a Steward proposal. They never write directly.
      </p>
      <div className="mt-3 overflow-x-auto">
        <table className="w-full min-w-[560px] text-left text-sm">
          <thead className="text-xs text-muted-foreground">
            <tr className="border-b border-border">
              <th className="pb-2 font-medium">Server</th>
              <th className="pb-2 font-medium">Status</th>
              {agents.map((agent) => (
                <th
                  key={agent}
                  className="pb-2 text-center font-medium capitalize"
                >
                  {agent}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr
                key={entry.id}
                className="border-b border-border/70 last:border-0"
              >
                <td className="py-3 pr-4">
                  <p className="font-medium">{entry.name}</p>
                  <p className="max-w-[340px] truncate text-xs text-muted-foreground">
                    {entry.description} · {entry.transport}
                  </p>
                </td>
                <td className="py-3 pr-3 text-xs capitalize text-muted-foreground">
                  {entry.status}
                </td>
                {agents.map((agent) => (
                  <td key={agent} className="py-3 text-center">
                    <button
                      type="button"
                      role="switch"
                      aria-label={`${entry.name} for ${agent}`}
                      aria-checked={entry.agents[agent]}
                      disabled={proposing}
                      onClick={() => onRequestToggle(entry, agent)}
                      className={`relative h-5 w-9 rounded-full transition-colors disabled:opacity-50 ${
                        entry.agents[agent] ? "bg-primary" : "bg-muted"
                      }`}
                    >
                      <span
                        className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${
                          entry.agents[agent]
                            ? "translate-x-4"
                            : "translate-x-0"
                        }`}
                      />
                    </button>
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
