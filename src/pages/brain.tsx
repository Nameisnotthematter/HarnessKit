import { Brain, Loader2, RefreshCw, TriangleAlert } from "lucide-react";
import { useEffect } from "react";
import { AgentBrainCard } from "@/components/brain/agent-brain-card";
import { McpRegistry } from "@/components/brain/mcp-registry";
import { SharedSkillsPanel } from "@/components/brain/shared-skills-panel";
import { StewardPanel } from "@/components/brain/steward-panel";
import {
  type BrainAgentId,
  type McpRegistryEntry,
  useBrainStore,
} from "@/stores/brain-store";

export default function BrainPage() {
  const {
    snapshot,
    proposals,
    messages,
    loading,
    proposing,
    approvingId,
    rejectingId,
    error,
    fetchSnapshot,
    propose,
    proposeMemoryEdit,
    approve,
    reject,
  } = useBrainStore();

  useEffect(() => {
    fetchSnapshot();
  }, [fetchSnapshot]);

  const requestMcpToggle = (entry: McpRegistryEntry, agent: BrainAgentId) => {
    const action = entry.agents[agent] ? "disable" : "enable";
    propose(
      `${action} MCP server "${entry.name}" for ${agent}. Keep other agents unchanged.`,
    );
  };

  return (
    <div className="h-full overflow-y-auto overscroll-contain p-4 sm:p-6">
      <header className="mb-5 flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <Brain size={22} className="text-primary" />
            <h1 className="text-xl font-semibold">Brain Steward</h1>
          </div>
          <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
            Inspect Codex, Hermes, and OpenClaw brains. All mutations require a
            reviewed proposal.
          </p>
        </div>
        <button
          type="button"
          onClick={fetchSnapshot}
          disabled={loading}
          className="flex items-center gap-1.5 rounded-lg border border-border px-3 py-1.5 text-xs hover:bg-muted disabled:opacity-50"
        >
          <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
          Refresh snapshot
        </button>
      </header>

      {error && (
        <div
          role="alert"
          className="mb-4 flex gap-2 rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive"
        >
          <TriangleAlert size={16} className="mt-0.5 shrink-0" />
          {error}
        </div>
      )}

      {loading && !snapshot ? (
        <div className="flex min-h-64 items-center justify-center gap-2 text-sm text-muted-foreground">
          <Loader2 size={16} className="animate-spin" /> Loading brain snapshot…
        </div>
      ) : snapshot ? (
        <div className="space-y-5">
          <section className="grid gap-4 md:grid-cols-2 2xl:grid-cols-3">
            {snapshot.agents.map((agent) => (
              <AgentBrainCard
                key={agent.id}
                agent={agent}
                proposing={proposing}
                onProposeMemoryEdit={proposeMemoryEdit}
              />
            ))}
          </section>

          <SharedSkillsPanel
            skills={snapshot.shared_skills}
            proposing={proposing}
            onRequestMigration={() =>
              propose(
                "Migrate personal skills to the shared canonical root. Stop on conflicts.",
              )
            }
          />
          <McpRegistry
            entries={snapshot.mcp_registry}
            proposing={proposing}
            onRequestToggle={requestMcpToggle}
          />
          <StewardPanel
            messages={messages}
            proposals={proposals}
            proposing={proposing}
            approvingId={approvingId}
            rejectingId={rejectingId}
            onPropose={propose}
            onApprove={approve}
            onReject={reject}
          />
        </div>
      ) : (
        <p className="py-12 text-center text-sm text-muted-foreground">
          Brain snapshot unavailable.
        </p>
      )}
    </div>
  );
}
