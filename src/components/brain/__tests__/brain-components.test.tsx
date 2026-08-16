import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AgentBrainCard } from "../agent-brain-card";
import { StewardPanel } from "../steward-panel";

describe("Brain Steward components", () => {
  it("marks agent memory as private and read-only", () => {
    render(
      <AgentBrainCard
        agent={{
          id: "codex",
          name: "Codex",
          status: "ready",
          config: [],
          persona: [],
          memory: [
            {
              path: "/tmp/MEMORY.md",
              label: "MEMORY.md",
              summary: "Curated memory",
              exists: true,
              read_only: true,
            },
          ],
        }}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Memory" }));
    expect(screen.getByText(/read-only/i)).toBeInTheDocument();
    expect(screen.getByText(/never shared/i)).toBeInTheDocument();
  });

  it("shows review evidence and only the approval write action", () => {
    render(
      <StewardPanel
        messages={[]}
        proposing={false}
        approvingId={null}
        onPropose={vi.fn()}
        onApprove={vi.fn()}
        proposals={[
          {
            id: "p1",
            title: "Align skill",
            summary: "Share one portable skill",
            diff: "+ skill = shared",
            risk: "medium",
            validations: [{ label: "Path check", status: "pass" }],
            status: "pending",
            created_at: "2026-08-15T00:00:00Z",
          },
        ]}
      />,
    );

    expect(screen.getByText("+ skill = shared")).toBeInTheDocument();
    expect(screen.getByText(/medium risk/i)).toBeInTheDocument();
    expect(screen.getByText("Path check")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Approve & apply" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /save/i }),
    ).not.toBeInTheDocument();
  });
});
