import { create } from "zustand";
import { humanizeError } from "@/lib/errors";
import { transport } from "@/lib/transport";

export type BrainAgentId = "codex" | "hermes" | "openclaw";
export type BrainSectionKind = "config" | "persona" | "memory";

export interface BrainFile {
  path: string;
  label: string;
  summary: string;
  exists: boolean;
  content?: string;
  read_only?: boolean;
}

export interface BrainAgent {
  id: BrainAgentId;
  name: string;
  version?: string;
  status: "ready" | "partial" | "offline";
  config: BrainFile[];
  persona: BrainFile[];
  memory: BrainFile[];
}

export interface SharedSkill {
  name: string;
  description: string;
  source: string;
  agents: BrainAgentId[];
  status: "ready" | "needs_setup" | "unavailable";
}

export interface McpRegistryEntry {
  id: string;
  name: string;
  description: string;
  transport: "stdio" | "http" | "sse";
  agents: Record<BrainAgentId, boolean>;
  status: "configured" | "disabled";
}

export interface StewardValidation {
  label: string;
  status: "pass" | "warning" | "fail";
  detail?: string;
}

export interface StewardProposal {
  id: string;
  title: string;
  summary: string;
  diff: string;
  risk: "low" | "medium" | "high";
  validations: StewardValidation[];
  status: "pending" | "approved" | "rejected" | "failed";
  created_at: string;
}

export interface BrainSnapshot {
  agents: BrainAgent[];
  shared_skills: SharedSkill[];
  mcp_registry: McpRegistryEntry[];
  proposals: StewardProposal[];
  captured_at: string;
}

export interface StewardMessage {
  id: string;
  role: "user" | "steward";
  content: string;
}

interface StewardChatReply {
  message: string;
  proposal?: StewardProposal;
}

interface BrainState {
  snapshot: BrainSnapshot | null;
  proposals: StewardProposal[];
  messages: StewardMessage[];
  loading: boolean;
  proposing: boolean;
  approvingId: string | null;
  rejectingId: string | null;
  error: string | null;
  fetchSnapshot: () => Promise<void>;
  propose: (prompt: string) => Promise<void>;
  proposeMemoryEdit: (
    agent: BrainAgentId,
    path: string,
    content: string,
  ) => Promise<boolean>;
  approve: (proposalId: string) => Promise<void>;
  reject: (proposalId: string) => Promise<void>;
}

function messageId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export const useBrainStore = create<BrainState>((set, get) => ({
  snapshot: null,
  proposals: [],
  messages: [],
  loading: false,
  proposing: false,
  approvingId: null,
  rejectingId: null,
  error: null,

  async fetchSnapshot() {
    set({ loading: true, error: null });
    try {
      const snapshot = await transport<BrainSnapshot>("brain_snapshot");
      set({
        snapshot,
        proposals: (snapshot.proposals ?? []).filter(
          (proposal) => proposal.status === "pending",
        ),
        loading: false,
      });
    } catch (error) {
      set({ error: humanizeError(error), loading: false });
    }
  },

  async propose(prompt) {
    const content = prompt.trim();
    if (!content || get().proposing) return;

    const history = get().messages.map((message) => ({
      role: message.role,
      content: message.content,
    }));

    const userMessage: StewardMessage = {
      id: messageId(),
      role: "user",
      content,
    };
    set((state) => ({
      proposing: true,
      error: null,
      messages: [...state.messages, userMessage],
    }));

    try {
      const reply = await transport<StewardChatReply>("steward_chat", {
        prompt: content,
        history,
      });
      set((state) => ({
        proposing: false,
        proposals: reply.proposal
          ? [reply.proposal, ...state.proposals]
          : state.proposals,
        messages: [
          ...state.messages,
          {
            id: messageId(),
            role: "steward",
            content: reply.message,
          },
        ],
      }));
    } catch (error) {
      set({ error: humanizeError(error), proposing: false });
    }
  },

  async proposeMemoryEdit(agent, path, content) {
    if (get().proposing) return false;
    set({ proposing: true, error: null });
    try {
      const proposal = await transport<StewardProposal>(
        "steward_propose_memory_edit",
        { agent, path, content },
      );
      set((state) => ({
        proposing: false,
        proposals: [proposal, ...state.proposals],
        messages: [
          ...state.messages,
          {
            id: messageId(),
            role: "steward",
            content: `Private memory edit proposal ready: ${proposal.title}`,
          },
        ],
      }));
      return true;
    } catch (error) {
      set({ error: humanizeError(error), proposing: false });
      return false;
    }
  },

  async approve(proposalId) {
    if (get().approvingId || get().rejectingId) return;
    set({ approvingId: proposalId, error: null });
    try {
      const approved = await transport<StewardProposal>("steward_approve", {
        proposalId,
      });
      set((state) => ({
        approvingId: null,
        proposals: state.proposals.filter(
          (proposal) => proposal.id !== proposalId,
        ),
        messages: [
          ...state.messages,
          {
            id: messageId(),
            role: "steward",
            content: `Applied approved proposal: ${approved.title}`,
          },
        ],
      }));
      await get().fetchSnapshot();
    } catch (error) {
      set({ error: humanizeError(error), approvingId: null });
    }
  },

  async reject(proposalId) {
    if (get().approvingId || get().rejectingId) return;
    set({ rejectingId: proposalId, error: null });
    try {
      const rejected = await transport<StewardProposal>("steward_reject", {
        proposalId,
      });
      set((state) => ({
        rejectingId: null,
        proposals: state.proposals.filter(
          (proposal) => proposal.id !== proposalId,
        ),
        messages: [
          ...state.messages,
          {
            id: messageId(),
            role: "steward",
            content: `Rejected proposal: ${rejected.title}`,
          },
        ],
      }));
    } catch (error) {
      set({ error: humanizeError(error), rejectingId: null });
    }
  },
}));
