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
      error: null,
    });
  });

  it("loads the snapshot with the dedicated command", async () => {
    vi.mocked(transport).mockResolvedValue(snapshot);
    await useBrainStore.getState().fetchSnapshot();
    expect(transport).toHaveBeenCalledWith("brain_snapshot");
    expect(useBrainStore.getState().snapshot).toEqual(snapshot);
  });

  it("creates a proposal without applying it", async () => {
    vi.mocked(transport).mockResolvedValue(proposal);
    await useBrainStore.getState().propose("enable data-agent");
    expect(transport).toHaveBeenCalledWith("steward_propose", {
      prompt: "enable data-agent",
    });
    expect(useBrainStore.getState().proposals).toEqual([proposal]);
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
  });
});
