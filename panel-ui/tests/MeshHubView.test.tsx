import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import React from "react";
import MeshHubView from "../src/components/Mesh/MeshHubView";

// Mock Tauri invoke & listen API
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation(async (cmd: string) => {
    if (cmd === "get_current_config_state") {
      return { has_openai: true, has_gemini: false, has_telegram: false };
    }
    if (cmd === "get_realtime_metrics") {
      return { cpu_percent: 15, ram_used_gb: 4, ram_total_gb: 16 };
    }
    return null;
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockReturnValue(Promise.resolve(() => {})),
}));

describe("MeshHubView", () => {
  it("renders correctly with navigation tabs and header", () => {
    render(<MeshHubView token="mock-token" />);

    expect(screen.getByText("Xavier Mesh Hub")).toBeInTheDocument();
    expect(screen.getByText("Active Network: P2P-Mesh-Mainnet")).toBeInTheDocument();

    // Verify all tabs exist
    expect(screen.getByRole("button", { name: /Networks/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Topology/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /DAO Governance/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /P2P Chat/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Family Health/i })).toBeInTheDocument();
  });

  it("switches tab content when navigation buttons are clicked", () => {
    render(<MeshHubView token="mock-token" />);

    // Click Topology tab
    fireEvent.click(screen.getByRole("button", { name: /Topology/i }));
    expect(screen.getByText("Mesh Network Topology")).toBeInTheDocument();

    // Click DAO Governance tab
    fireEvent.click(screen.getByRole("button", { name: /DAO Governance/i }));
    expect(screen.getByText("DAO Governance & Tokenomics")).toBeInTheDocument();

    // Click P2P Chat tab
    fireEvent.click(screen.getByRole("button", { name: /Chat/i }));
    expect(screen.getByText("Encrypted Mesh P2P Chat")).toBeInTheDocument();

    // Click Family Health tab
    fireEvent.click(screen.getByRole("button", { name: /Family Health/i }));
    expect(screen.getByText("Family Node Health & Auto-Repair Module")).toBeInTheDocument();
  });

  it("triggers onClose callback when back button is clicked", () => {
    const handleClose = vi.fn();
    render(<MeshHubView token="mock-token" onClose={handleClose} />);

    const backButton = screen.getByRole("button", { name: /Back to Main View/i });
    fireEvent.click(backButton);
    expect(handleClose).toHaveBeenCalledTimes(1);
  });
});
