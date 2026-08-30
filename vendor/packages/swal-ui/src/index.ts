/** Shared SWAL UI primitives for backoffice + product shells */
export type NavItem = {
  id: string;
  label: string;
  href: string;
  icon?: string;
  appId?: string;
};

export type AppRegistryEntry = {
  appId: string;
  name: string;
  description: string;
  monorepoPath: string;
  kind: "product" | "infra" | "platform";
  health?: {
    xavierNamespace?: string;
    meshNamespacePrefix?: string;
    localUrl?: string;
  };
  modules?: string[];
};

export const SWAL_NAV_DEFAULT: NavItem[] = [
  { id: "overview", label: "Overview", href: "/" },
  { id: "council", label: "Consejo", href: "/council" },
  { id: "proposals", label: "Propuestas", href: "/proposals" },
  { id: "backlog", label: "Backlog", href: "/backlog" },
  { id: "support", label: "Soporte", href: "/support" },
  { id: "nodes", label: "Nodos & meshes", href: "/nodes" },
  { id: "params", label: "Params", href: "/params" },
  { id: "node", label: "SWAL Node", href: "/node" },
  { id: "onboarding", label: "Onboarding", href: "/onboarding" },
  { id: "xavier", label: "Xavier Memory", href: "/xavier" },
  { id: "xavier-project", label: "Xavier Project", href: "/xavier-project" },
  { id: "mesh", label: "Mesh", href: "/mesh" },
  { id: "apps", label: "Apps registry", href: "/apps" },
  { id: "scores", label: "GitCore scores", href: "/scores" },
  { id: "wallet", label: "$SWAL Wallet", href: "/wallet" },
  { id: "verify", label: "Feature verify", href: "/verify" },
];

// Primitives & Hive components
export { default as Button } from "./components/Button.svelte";
export { default as Card } from "./components/Card.svelte";
export { default as Badge } from "./components/Badge.svelte";
export { default as Input } from "./components/Input.svelte";
export { default as Table } from "./components/Table.svelte";
export { default as Tabs } from "./components/Tabs.svelte";
export { default as Skeleton } from "./components/Skeleton.svelte";
export { default as Modal } from "./components/Modal.svelte";

export { default as StatusBadge } from "./components/StatusBadge.svelte";
export { default as LoadingState } from "./components/LoadingState.svelte";
export { default as Terminal } from "./components/Terminal.svelte";
export { default as CommandPalette } from "./components/CommandPalette.svelte";
export { default as Toaster } from "./components/Toaster.svelte";
export { default as LogViewer } from "./components/LogViewer.svelte";
export { default as ConfigEditor } from "./components/ConfigEditor.svelte";

export { default as CommitGraph } from "./components/CommitGraph.svelte";
export { default as ConnectionBadge } from "./components/ConnectionBadge.svelte";
export { default as LiveToast } from "./components/LiveToast.svelte";
export { default as GraphExplorer } from "./components/GraphExplorer.svelte";
export { default as TimelineView } from "./components/TimelineView.svelte";

// Modelos (Wave-MC-F4)
export { default as ModelBadge } from "./components/ModelBadge.svelte";
export { default as ModelSelector } from "./components/ModelSelector.svelte";
export { default as ChallengeCard } from "./components/ChallengeCard.svelte";
export { default as ChallengeForm } from "./components/ChallengeForm.svelte";
export { default as ChallengePanel } from "./components/ChallengePanel.svelte";

export { setXavierBaseUrlResolver } from "./lib/config";
