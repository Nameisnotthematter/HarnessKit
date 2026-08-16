import { CheckCircle2, CircleOff, FileText, LockKeyhole } from "lucide-react";
import { useState } from "react";
import type { BrainAgent, BrainSectionKind } from "@/stores/brain-store";

const sections: { id: BrainSectionKind; label: string }[] = [
  { id: "config", label: "Config" },
  { id: "persona", label: "Persona" },
  { id: "memory", label: "Memory" },
];

export function AgentBrainCard({ agent }: { agent: BrainAgent }) {
  const [section, setSection] = useState<BrainSectionKind>("config");
  const files = agent[section];

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
            Read-only. Memory is private to this agent and never shared.
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
              </div>
              <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
                {file.summary}
              </p>
              <p className="mt-1.5 truncate font-mono text-[10px] text-muted-foreground/80">
                {file.path}
              </p>
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
