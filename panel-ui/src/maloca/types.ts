/** Maloca domain types — mirror Xavier `src/maloca/types.rs` / `@swal/maloca-client`. */

export type SupportTicket = {
  id: string;
  title: string;
  body: string;
  status: string;
  created_at: string;
  feature_id?: string | null;
};

export type MicroTask = {
  id: string;
  parent_feature: string;
  kind: string;
  title: string;
  acceptance: string;
  evidence_paths?: string[];
  reward_hint: number;
  difficulty: number;
  status: string;
};

export type MeshTicketOffer = {
  id: string;
  microtask: MicroTask;
  offered_at: string;
  expires_at: string;
  claimed_by?: string | null;
};

export type MalocaPack = {
  generated_at: string;
  features_total: number;
  features_draft: number;
  gaps_zero_symbol_modules: string[];
  decisions_count: number;
  support_open: number;
  inbox_open: number;
};

export type Proposal = {
  id: string;
  type: string;
  title: string;
  body: string;
  status: "open" | "closed" | "reconsidering" | "analyzing";
  created_at: string;
  locked_param?: boolean;
};

export type ManagerAction = {
  id: string;
  type: "request_reconsideration" | "request_scenario_analysis";
  proposalId: string;
  reason: string;
  created_at: string;
};

export type NetworkParam = {
  key: string;
  default: string;
  locked_until_quorum: boolean;
  notes: string;
};

export type MeshSnapshot = {
  mode: string;
  genesis_node_id: string;
  parent_nodes_enabled: boolean;
  manager_adds_vote_weight: boolean;
  wallet_multi_node_anchor: boolean;
  nodes: {
    node_id: string;
    role: string;
    note: string;
    karma: number;
    active: boolean;
  }[];
  meshes: { id: string; kind: string; description: string }[];
};

export type VoteChoice = "yes" | "no" | "abstain";

export type Vote = {
  id: string;
  proposal_id: string;
  node_id: string;
  choice: VoteChoice;
  weight: number;
  created_at: string;
};

export type DecisionKind =
  | "proposal_created"
  | "voted"
  | "manager_request_reconsideration"
  | "manager_request_scenario_analysis"
  | "closed";

export type DecisionEvent = {
  id: string;
  kind: DecisionKind;
  proposal_id?: string | null;
  actor_node_id: string;
  genesis_node_id: string;
  payload: unknown;
  created_at: string;
};

export type NodeRecord = {
  node_id: string;
  role: string;
  karma: number;
  active: boolean;
  note: string;
};

export type MalocaSectionId =
  | "council"
  | "proposals"
  | "backlog"
  | "inbox"
  | "support"
  | "nodes"
  | "params"
  | "decisions"
  | "docs";

export type MalocaTheme = {
  bg: string;
  bgElevated: string;
  bgMuted: string;
  text: string;
  textMuted: string;
  accent: string;
  accentSoft: string;
  border: string;
  warning: string;
  warningBg: string;
  danger: string;
  radius: string;
  space: string;
  font: string;
  fontMono: string;
};

export type MalocaUiConfig = {
  version: number;
  theme: MalocaTheme;
  copy: {
    title: string;
    subtitle: string;
    managerNote: string;
  };
  layout: {
    sections: { id: MalocaSectionId; label: string; enabled: boolean }[];
  };
  editMode: {
    allowReorder: boolean;
    allowThemeEdit: boolean;
    lockedKeys: string[];
  };
};
