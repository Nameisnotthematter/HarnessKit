import {
  CheckCircle2,
  CircleOff,
  FileText,
  LockKeyhole,
  Pencil,
} from "lucide-react";
import { useState } from "react";
import type { BrainAgent, BrainSectionKind } from "@/stores/brain-store";

const sections: { id: BrainSectionKind; label: string }[] = [
  { id: "config", label: "Config" },
  { id: "persona", label: "Persona" },
  { id: "memory", label: "Memory" },
];

interface AgentBrainCardProps {
  agent: BrainAgent;
  proposing?: boolean;
  onProposeMemoryEdit?: (
    agent: BrainAgent["id"],
    path: string,
    content: string,
  ) => Promise<boolean>;
}

export function AgentBrainCard({
  agent,
  proposing = false,
  onProposeMemoryEdit,
}: AgentBrainCardProps) {
  const [section, setSection] = useState<BrainSectionKind>("config");
  const [editingPath, setEditingPath] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const files = agent[section];

  const beginMemoryEdit = (path: string, content: string) => {
    setEditingPath(path);
    setDraft(content);
  };

  const createMemoryProposal = async () => {
    if (!editingPath || !onProposeMemoryEdit) return;
    const created = await onProposeMemoryEdit(agent.id, editingPath, draft);
    if (created) {
      setEditingPath(null);
      setDraft("");
    }
  };

  return (
    <article className="min-w-0 rounded-xl border border-border bg-card shadow-sm">
      <header className="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
        <div>
          <h2 className="font-semibold text-foreground">{agent.name}</h2>
          <p className="mt-0.5 text-xs text-muted-foreground">
            {agent.version ?? "Version unavailable"}
          </p>
        </div>
        <span className="flex items-center gap-1 rounded-full bg-muted px-2 py-1 text-[11px] text-muted-foreground">
          {agent.status === "ready" ? (
            <CheckCircle2 size={12} className="text-emerald-500" />
          ) : (
            <CircleOff size={12} />
          )}
          {agent.status}
        </span>
      </header>

      <div
        className="flex gap-1 border-b border-border px-3 pt-2"
        role="tablist"
      >
        {sections.map((item) => (
          <button
            key={item.id}
            type="button"
            role="tab"
            aria-selected={section === item.id}
            onClick={() => setSection(item.id)}
            className={`rounded-t-md px-2.5 py-1.5 text-xs transition-colors ${
              section === item.id
                ? "bg-muted font-medium text-foreground"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {item.label}
          </button>
        ))}
      </div>

      {section === "memory" && (
        <div className="mx-3 mt-3 flex gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 p-2.5 text-xs text-amber-700 dark:text-amber-300">
          <LockKeyhole size={14} className="mt-0.5 shrink-0" />
          <span>
            Private and isolated. Memory stays with this agent and is never
            shared. Edits require approval.
          </span>
        </div>
      )}

      <div className="space-y-2 p-3">
        {files.length === 0 ? (
          <p className="py-5 text-center text-xs text-muted-foreground">
            No files detected.
          </p>
        ) : (
          files.map((file) => (
            <div
              key={file.path}
              className="rounded-lg border border-border p-2.5"
            >
              <div className="flex min-w-0 items-center gap-2">
                <FileText
                  size={14}
                  className="shrink-0 text-muted-foreground"
                />
                <span className="truncate text-sm font-medium">
                  {file.label}
                </span>
                {!file.exists && (
                  <span className="ml-auto text-[10px] text-muted-foreground">
                    missing
                  </span>
                )}
                {section === "memory" && file.exists && !file.read_only && (
                  <button
                    type="button"
                    disabled={proposing}
                    onClick={() =>
                      beginMemoryEdit(file.path, file.content ?? "")
                    }
                    className="ml-auto flex items-center gap-1 rounded-md border border-border px-2 py-1 text-[11px] hover:bg-muted disabled:opacity-50"
                  >
                    <Pencil size={11} /> Edit
                  </button>
                )}
              </div>
              <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
                {file.summary}
              </p>
              <p className="mt-1.5 truncate font-mono text-[10px] text-muted-foreground/80">
                {file.path}
              </p>
              {section === "memory" && editingPath === file.path && (
                <div className="mt-2 space-y-2 rounded-lg border border-border bg-muted/30 p-2">
                  <textarea
                    aria-label={`Edit ${file.label}`}
                    value={draft}
                    disabled={proposing}
                    onChange={(event) => setDraft(event.target.value)}
                    rows={8}
                    className="w-full resize-y rounded-md border border-border bg-background p-2 font-mono text-xs outline-none focus:ring-2 focus:ring-primary/30 disabled:opacity-50"
                  />
                  <div className="flex justify-end gap-2">
                    <button
                      type="button"
                      disabled={proposing}
                      onClick={() => {
                        setEditingPath(null);
                        setDraft("");
                      }}
                      className="rounded-md border border-border px-2.5 py-1.5 text-xs hover:bg-muted disabled:opacity-50"
                    >
                      Cancel
                    </button>
                    <button
                      type="button"
                      disabled={proposing}
                      onClick={createMemoryProposal}
                      className="rounded-md bg-primary px-2.5 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
                    >
                      Create proposal
                    </button>
                  </div>
                </div>
              )}
              {file.content && (
                <details className="mt-2">
                  <summary className="cursor-pointer text-xs font-medium text-primary">
                    View content
                  </summary>
                  <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap rounded-md bg-muted/60 p-2 font-mono text-[10px] leading-relaxed">
                    {file.content}
                  </pre>
                </details>
              )}
            </div>
          ))
        )}
      </div>
    </article>
  );
}
