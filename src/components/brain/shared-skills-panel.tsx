import { Wrench } from "lucide-react";
import type { SharedSkill } from "@/stores/brain-store";

export function SharedSkillsPanel({
  skills,
  proposing,
  onRequestMigration,
}: {
  skills: SharedSkill[];
  proposing: boolean;
  onRequestMigration: () => void;
}) {
  const needsSetup = skills.some((skill) => skill.status === "needs_setup");
  return (
    <section className="rounded-xl border border-border bg-card p-4 shadow-sm">
      <div className="flex items-center gap-2">
        <Wrench size={16} className="text-primary" />
        <h2 className="font-semibold">Shared Skills</h2>
        <span className="ml-auto text-xs text-muted-foreground">
          {skills.filter((skill) => skill.status === "ready").length}/
          {skills.length} ready
        </span>
        {needsSetup && (
          <button
            type="button"
            disabled={proposing}
            onClick={onRequestMigration}
            className="rounded-lg border border-border px-2.5 py-1 text-xs font-medium hover:bg-muted disabled:opacity-50"
          >
            Prepare migration
          </button>
        )}
      </div>
      <p className="mt-1 text-xs text-muted-foreground">
        Portable instructions and tools available across agents.
      </p>
      <div className="mt-3 grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
        {skills.map((skill) => (
          <div
            key={`${skill.source}:${skill.name}`}
            className="rounded-lg border border-border p-3"
          >
            <div className="flex items-center gap-2">
              <span className="truncate text-sm font-medium">{skill.name}</span>
              <span
                className={`ml-auto h-2 w-2 shrink-0 rounded-full ${
                  skill.status === "ready"
                    ? "bg-emerald-500"
                    : skill.status === "needs_setup"
                      ? "bg-amber-500"
                      : "bg-muted-foreground"
                }`}
                aria-hidden="true"
              />
              <span className="sr-only">{skill.status}</span>
            </div>
            <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
              {skill.description}
            </p>
            <p className="mt-2 text-[10px] uppercase tracking-wide text-muted-foreground">
              {skill.agents.join(" · ")}
            </p>
          </div>
        ))}
      </div>
    </section>
  );
}
