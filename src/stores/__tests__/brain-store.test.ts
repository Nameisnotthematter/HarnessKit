import { beforeEach, describe, expect, it, vi } from "vitest";
import { transport } from "@/lib/transport";
import {
  type BrainSnapshot,
  type StewardProposal,
  useBrainStore,
} from "../brain-store";

vi.mock("@/lib/transport", () => ({ transport: vi.fn() }));

const proposal: StewardProposal = {
  id: "p1",
  title: "Enable MCP",
  summary: "Enable data-agent for Codex",
  diff: "+ enabled = true",
  risk: "low",
  validations: [{ label: "Schema", status: "pass" }],
  status: "pending",
  created_at: "2026-08-15T00:00:00Z",
};

const snapshot: BrainSnapshot = {
  agents: [],
  shared_skills: [],
  mcp_registry: [],
  proposals: [],
  captured_at: "2026-08-15T00:00:00Z",
};

describe("brain-store", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useBrainStore.setState({
      snapshot: null,
      proposals: [],
      messages: [],
      loading: false,
      proposing: false,
      approvingId: null,
      rejectingId: null,
      error: null,
    });
  });

  it("loads the snapshot with the dedicated command", async () => {
    const approved = { ...proposal, id: "p2", status: "approved" as const };
    const rejected = { ...proposal, id: "p3", status: "rejected" as const };
    const loadedSnapshot = {
      ...snapshot,
      proposals: [proposal, approved, rejected],
    };
    vi.mocked(transport).mockResolvedValue(loadedSnapshot);
    await useBrainStore.getState().fetchSnapshot();
    expect(transport).toHaveBeenCalledWith("brain_snapshot");
    expect(useBrainStore.getState().snapshot).toEqual(loadedSnapshot);
    expect(useBrainStore.getState().proposals).toEqual([proposal]);
  });

  it("accepts a natural chat reply without creating a proposal", async () => {
    vi.mocked(transport).mockResolvedValue({
      message: "Codex uses codegraph.",
    });
    await useBrainStore.getState().propose("Which MCP does Codex use?");
    expect(transport).toHaveBeenCalledWith("steward_chat", {
      prompt: "Which MCP does Codex use?",
      history: [],
    });
    expect(useBrainStore.getState().proposals).toEqual([]);
    const messages = useBrainStore.getState().messages;
    expect(messages[messages.length - 1]?.content).toBe("Codex uses codegraph.");
  });

  it("adds an optional chat proposal without applying it", async () => {
    vi.mocked(transport).mockResolvedValue({
      message: "I prepared a reviewable proposal.",
      proposal,
    });
    await useBrainStore.getState().propose("enable data-agent");
    expect(transport).toHaveBeenCalledWith("steward_chat", {
      prompt: "enable data-agent",
      history: [],
    });
    expect(useBrainStore.getState().proposals).toEqual([proposal]);
  });

  it("creates a private memory proposal with the exact edit payload", async () => {
    const memoryProposal = {
      ...proposal,
      id: "memory-p1",
      title: "Edit MEMORY.md",
    };
    vi.mocked(transport).mockResolvedValue(memoryProposal);
    useBrainStore.setState({ proposals: [proposal] });

    const created = await useBrainStore
      .getState()
      .proposeMemoryEdit("hermes", "/private/MEMORY.md", "exact\ncontent\n");

    expect(created).toBe(true);
    expect(transport).toHaveBeenCalledWith("steward_propose_memory_edit", {
      agent: "hermes",
      path: "/private/MEMORY.md",
      content: "exact\ncontent\n",
    });
    expect(useBrainStore.getState().proposals).toEqual([
      memoryProposal,
      proposal,
    ]);
  });

  it("applies only through steward_approve and refreshes", async () => {
    const approved = { ...proposal, status: "approved" as const };
    useBrainStore.setState({ proposals: [proposal] });
    vi.mocked(transport)
      .mockResolvedValueOnce(approved)
      .mockResolvedValueOnce({ ...snapshot, proposals: [approved] });

    await useBrainStore.getState().approve("p1");

    expect(transport).toHaveBeenNthCalledWith(1, "steward_approve", {
      proposalId: "p1",
    });
    expect(transport).toHaveBeenNthCalledWith(2, "brain_snapshot");
    expect(useBrainStore.getState().proposals).toEqual([]);
  });

  it("rejects through steward_reject without refreshing brain files", async () => {
    const rejected = { ...proposal, status: "rejected" as const };
    useBrainStore.setState({ proposals: [proposal] });
    vi.mocked(transport).mockResolvedValue(rejected);

    await useBrainStore.getState().reject("p1");

    expect(transport).toHaveBeenCalledOnce();
    expect(transport).toHaveBeenCalledWith("steward_reject", {
      proposalId: "p1",
    });
    expect(useBrainStore.getState().proposals).toEqual([]);
    const messages = useBrainStore.getState().messages;
    expect(messages[messages.length - 1]?.content).toBe(
      "Rejected proposal: Enable MCP",
    );
  });
});
