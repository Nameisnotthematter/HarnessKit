import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AgentBrainCard } from "../agent-brain-card";
import { StewardPanel } from "../steward-panel";

describe("Brain Steward components", () => {
  it("keeps memory private and creates an edit proposal instead of saving", async () => {
    const onProposeMemoryEdit = vi.fn().mockResolvedValue(true);
    render(
      <AgentBrainCard
        onProposeMemoryEdit={onProposeMemoryEdit}
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
              content: "original memory",
              read_only: false,
            },
          ],
        }}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Memory" }));
    expect(screen.getByText(/private and isolated/i)).toBeInTheDocument();
    expect(screen.getByText(/never shared/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /edit/i }));
    const editor = screen.getByRole("textbox", { name: "Edit MEMORY.md" });
    expect(editor).toHaveValue("original memory");
    fireEvent.change(editor, { target: { value: "updated private memory" } });
    fireEvent.click(screen.getByRole("button", { name: "Create proposal" }));

    expect(onProposeMemoryEdit).toHaveBeenCalledWith(
      "codex",
      "/tmp/MEMORY.md",
      "updated private memory",
    );
    await waitFor(() =>
      expect(screen.queryByRole("textbox")).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: /copy|share/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /save/i }),
    ).not.toBeInTheDocument();
  });

  it("cancels memory editing and disables edit controls while proposing", () => {
    const agent = {
      id: "openclaw" as const,
      name: "OpenClaw",
      status: "ready" as const,
      config: [],
      persona: [],
      memory: [
        {
          path: "/tmp/MEMORY.md",
          label: "MEMORY.md",
          summary: "Private memory",
          exists: true,
          content: "memory",
        },
      ],
    };
    const { rerender } = render(
      <AgentBrainCard agent={agent} onProposeMemoryEdit={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Memory" }));
    fireEvent.click(screen.getByRole("button", { name: /edit/i }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();

    rerender(
      <AgentBrainCard agent={agent} proposing onProposeMemoryEdit={vi.fn()} />,
    );
    expect(screen.getByRole("button", { name: /edit/i })).toBeDisabled();
  });

  it("does not offer editing for read-only memory", () => {
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
              path: "/tmp/large-memory.md",
              label: "large-memory.md",
              summary: "Private, over 256 KiB limit",
              exists: true,
              content: "preview only",
              read_only: true,
            },
          ],
        }}
        onProposeMemoryEdit={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Memory" }));
    expect(
      screen.queryByRole("button", { name: /edit/i }),
    ).not.toBeInTheDocument();
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
