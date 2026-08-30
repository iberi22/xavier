import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CreateNetworkWizard } from "../src/components/Mesh/CreateNetworkWizard";
import { DaoGovernancePanel } from "../src/components/Mesh/DaoGovernancePanel";
import { MeshTopologyGraph } from "../src/components/Mesh/MeshTopologyGraph";

describe("Mesh E2E UI Components", () => {
  it("renders CreateNetworkWizard with template selectors", () => {
    render(<CreateNetworkWizard isOpen={true} onClose={() => {}} onCreated={() => {}} />);
    expect(screen.getByText(/Network Name/i)).toBeInTheDocument();
  });

  it("renders MeshTopologyGraph with master and member nodes", () => {
    render(<MeshTopologyGraph networkId="test-net" onDisconnectNode={() => {}} />);
    expect(screen.getByText(/Master Host/i)).toBeInTheDocument();
  });

  it("renders DaoGovernancePanel with proposals and ballot buttons", () => {
    render(<DaoGovernancePanel onVote={() => {}} />);
    expect(screen.getByText(/DAO Governance/i)).toBeInTheDocument();
  });
});
