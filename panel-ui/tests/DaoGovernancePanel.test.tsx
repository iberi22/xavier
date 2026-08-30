import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import React from "react";
import DaoGovernancePanel, {
  Proposal,
} from "../src/components/Mesh/DaoGovernancePanel";

describe("DaoGovernancePanel Component", () => {
  const mockProposals: Proposal[] = [
    {
      id: "prop-test-101",
      title: "Test Protocol Parameter Tuning",
      description: "Adjust P2P block propagation timeout parameter.",
      status: "active",
      authorNode: "node-validator-alpha",
      requiredEndorsement: "validator",
      expiresAt: new Date(Date.now() + 86400000).toISOString(),
      votes: { for: 30, against: 10, abstain: 5 },
      quorum: { current: 45, required: 50 },
      userVote: null,
    },
    {
      id: "prop-test-102",
      title: "Security Clearance Policy Revision",
      description: "Update default clearance rules for untrusted peers.",
      status: "passed",
      authorNode: "node-[#39ff14]-admin",
      requiredEndorsement: "security-lead",
      expiresAt: new Date(Date.now() - 86400000).toISOString(),
      votes: { for: 90, against: 10, abstain: 0 },
      quorum: { current: 100, required: 60 },
      userVote: "for",
    },
  ];

  it("renders panel header and proposal list with metadata", () => {
    render(
      <DaoGovernancePanel
        proposals={mockProposals}
        currentNodeId="node-local-test"
        currentNodeEndorsements={["validator", "security-lead"]}
      />
    );

    expect(screen.getByText("DAO Governance & Voting Center")).toBeInTheDocument();
    expect(screen.getByText("node-local-test")).toBeInTheDocument();
    expect(screen.getByText("Test Protocol Parameter Tuning")).toBeInTheDocument();
    expect(screen.getByText("node-validator-alpha")).toBeInTheDocument();
    expect(screen.getByText("validator")).toBeInTheDocument();
    expect(screen.getByText("Security Clearance Policy Revision")).toBeInTheDocument();
  });

  it("renders quorum progress and approval percentage progress bars", () => {
    render(
      <DaoGovernancePanel
        proposals={mockProposals}
        currentNodeId="node-local-test"
        currentNodeEndorsements={["validator", "security-lead"]}
      />
    );

    // Quorum calculations: 45/50 votes (90%)
    expect(screen.getByText(/45 \/ 50 votes \(90%\)/i)).toBeInTheDocument();

    // Approval ratio calculations: 30 For / 10 Against (67% Approval)
    expect(screen.getByText(/67% Approval \(30 For \/ 10 Against\)/i)).toBeInTheDocument();
  });

  it("allows interactive voting and invokes onVote callback", async () => {
    const handleVote = vi.fn();
    render(
      <DaoGovernancePanel
        proposals={mockProposals}
        currentNodeId="node-local-test"
        currentNodeEndorsements={["validator", "security-lead"]}
        onVote={handleVote}
      />
    );

    const voteForBtn = screen.getByRole("button", {
      name: /Vote For on Test Protocol Parameter Tuning/i,
    });
    expect(voteForBtn).toBeEnabled();

    fireEvent.click(voteForBtn);

    await waitFor(() => {
      expect(handleVote).toHaveBeenCalledWith("prop-test-101", "for");
    });
  });

  it("disables voting buttons when node lacks required endorsement (Anti-Hallucination Guard)", () => {
    render(
      <DaoGovernancePanel
        proposals={mockProposals}
        currentNodeId="node-unprivileged"
        currentNodeEndorsements={["read-only"]} // Lacks 'validator' and 'security-lead'
      />
    );

    // Should display warning notification elements
    const warnings = screen.getAllByText(
      /Voting Disabled: Current node lacks required endorsement/i
    );
    expect(warnings.length).toBeGreaterThan(0);

    const voteForBtn = screen.getByRole("button", {
      name: /Vote For on Test Protocol Parameter Tuning/i,
    });
    const voteAgainstBtn = screen.getByRole("button", {
      name: /Vote Against on Test Protocol Parameter Tuning/i,
    });
    const abstainBtn = screen.getByRole("button", {
      name: /Abstain vote on Test Protocol Parameter Tuning/i,
    });

    expect(voteForBtn).toBeDisabled();
    expect(voteAgainstBtn).toBeDisabled();
    expect(abstainBtn).toBeDisabled();
  });

  it("filters proposals by search query", () => {
    render(
      <DaoGovernancePanel
        proposals={mockProposals}
        currentNodeId="node-local-test"
        currentNodeEndorsements={["validator", "security-lead"]}
      />
    );

    const searchInput = screen.getByPlaceholderText(
      /Search proposals by title, description, or author node/i
    );

    fireEvent.change(searchInput, { target: { value: "Security Clearance" } });

    expect(screen.getByText("Security Clearance Policy Revision")).toBeInTheDocument();
    expect(screen.queryByText("Test Protocol Parameter Tuning")).not.toBeInTheDocument();
  });

  it("calls onRefresh when refresh button is clicked", () => {
    const handleRefresh = vi.fn();
    render(
      <DaoGovernancePanel
        proposals={mockProposals}
        currentNodeId="node-local-test"
        currentNodeEndorsements={["validator"]}
        onRefresh={handleRefresh}
      />
    );

    const refreshBtn = screen.getByRole("button", { name: /Refresh proposals/i });
    fireEvent.click(refreshBtn);

    expect(handleRefresh).toHaveBeenCalledTimes(1);
  });
});
