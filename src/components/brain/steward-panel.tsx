import { Check, Loader2, MessageSquareText, ShieldAlert } from "lucide-react";
import { useState } from "react";
import type { StewardMessage, StewardProposal } from "@/stores/brain-store";

interface StewardPanelProps {
  messages: StewardMessage[];
  proposals: StewardProposal[];
  proposing: boolean;
  approvingId: string | null;
  onPropose: (prompt: string) => void;
  onApprove: (proposalId: string) => void;
}

export function StewardPanel({
  messages,
  proposals,
  proposing,
  approvingId,
  onPropose,
  onApprove,
}: StewardPanelProps) {
  const [prompt, setPrompt] = useState("");

  const submit = () => {
    if (!prompt.trim()) return;
    onPropose(prompt);
    setPrompt("");
  };

  return (
    <section className="rounded-xl border border-border bg-card p-4 shadow-sm">
      <div className="flex items-center gap-2">
        <MessageSquareText size={16} className="text-primary" />
        <h2 className="font-semibold">Brain Steward</h2>
      </div>
      <p className="mt-1 text-xs text-muted-foreground">
        Describe a change. Steward returns a reviewable proposal before anything
        is written.
      </p>

      {messages.length > 0 && (
        <div className="mt-3 max-h-40 space-y-2 overflow-y-auto rounded-lg bg-muted/40 p-3">
          {messages.map((message) => (
            <div key={message.id} className="text-xs">
              <span className="mr-2 font-medium capitalize">
                {message.role}
              </span>
              <span className="text-muted-foreground">{message.content}</span>
            </div>
          ))}
        </div>
      )}

      <div className="mt-3 flex flex-col gap-2 sm:flex-row">
        <textarea
          value={prompt}
          onChange={(event) => setPrompt(event.target.value)}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === "Enter")
              submit();
          }}
          rows={2}
          placeholder="Ask Steward to align a skill, persona rule, config, or MCP connection…"
          className="min-h-16 flex-1 resize-y rounded-lg border border-border bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-primary/30"
        />
        <button
          type="button"
          onClick={submit}
          disabled={proposing || !prompt.trim()}
          className="self-stretch rounded-lg border border-border px-4 py-2 text-sm font-medium hover:bg-muted disabled:opacity-50 sm:self-end"
        >
          {proposing && (
            <Loader2 size={13} className="mr-1 inline animate-spin" />
          )}
          提交审批
        </button>
      </div>

      <div className="mt-4 space-y-3">
        {proposals.map((proposal) => (
          <article
            key={proposal.id}
            className="rounded-lg border border-border p-3"
          >
            <div className="flex flex-wrap items-start gap-2">
              <div className="min-w-0 flex-1">
                <h3 className="text-sm font-semibold">{proposal.title}</h3>
                <p className="mt-1 text-xs text-muted-foreground">
                  {proposal.summary}
                </p>
              </div>
              <span className="flex items-center gap-1 rounded-full bg-muted px-2 py-1 text-[11px] uppercase">
                <ShieldAlert size={11} /> {proposal.risk} risk
              </span>
            </div>

            <div className="mt-3 grid gap-3 lg:grid-cols-[minmax(0,1fr)_240px]">
              <pre className="max-h-64 overflow-auto rounded-lg bg-muted/60 p-3 font-mono text-[11px] leading-relaxed">
                {proposal.diff}
              </pre>
              <div className="space-y-2">
                <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  Validation
                </p>
                {proposal.validations.map((validation) => (
                  <div key={validation.label} className="flex gap-2 text-xs">
                    <Check
                      size={13}
                      className={
                        validation.status === "pass"
                          ? "text-emerald-500"
                          : validation.status === "warning"
                            ? "text-amber-500"
                            : "text-destructive"
                      }
                    />
                    <div>
                      <p>{validation.label}</p>
                      {validation.detail && (
                        <p className="text-muted-foreground">
                          {validation.detail}
                        </p>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            </div>

            <div className="mt-3 flex justify-end">
              {proposal.status === "pending" ? (
                <button
                  type="button"
                  onClick={() => onApprove(proposal.id)}
                  disabled={approvingId !== null}
                  className="flex items-center gap-1.5 rounded-lg bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
                >
                  {approvingId === proposal.id && (
                    <Loader2 size={12} className="animate-spin" />
                  )}
                  Approve &amp; apply
                </button>
              ) : (
                <span className="text-xs capitalize text-muted-foreground">
                  {proposal.status}
                </span>
              )}
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
