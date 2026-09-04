import { LayoutDashboard, Globe, ListTodo, ShieldAlert, Cpu, Landmark, MessageSquare } from "lucide-react";
import React from "react";

export type MalocaTabId =
  | "overview"
  | "registry"
  | "governance"
  | "support"
  | "backlog"
  | "challenges"
  | "models";

export interface TabConfig {
  id: MalocaTabId;
  label: string;
  icon: React.ElementType;
  description: string;
}

export const TABS: TabConfig[] = [
  {
    id: "overview",
    label: "Hub Overview",
    icon: LayoutDashboard,
    description: "Maloca ops workspace host & primary node status.",
  },
  {
    id: "registry",
    label: "Mesh Registry",
    icon: Globe,
    description: "Distributed P2P node directory and ecosystem topology.",
  },
  {
    id: "governance",
    label: "DAO Governance",
    icon: Landmark,
    description: "Network parameters, council voting, and consensus proposals.",
  },
  {
    id: "support",
    label: "Support Desk",
    icon: MessageSquare,
    description: "Active community support tickets and incident tracking.",
  },
  {
    id: "backlog",
    label: "Global Backlog",
    icon: ListTodo,
    description: "Unified cross-node task queue & work-item scheduling.",
  },
];
