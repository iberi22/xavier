import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import {
  MeshTopologyGraph,
  buildTopologyNodes,
} from "../src/components/Mesh/MeshTopologyGraph";
import type { MeshPeer } from "../src/types";

describe("MeshTopologyGraph", () => {
  const samplePeers: MeshPeer[] = [
    {
      node_id: "peer-employee-1",
      alias: "Dev Laptop 01",
      endpoint_url: "http://192.168.1.50:8006",
      role: "editor",
      clearance: "secret",
      last_seen_at: Date.now() / 1000 - 30,
      sync_enabled: true,
    },
    {
      node_id: "peer-storage-2",
      alias: "Storage Node East",
      endpoint_url: "http://192.168.1.99:8006",
      role: "admin",
      clearance: "top_secret",
      last_seen_at: Date.now() / 1000 - 10,
      sync_enabled: true,
    },
  ];

  it("builds correct node hierarchy from peers", () => {
    const nodes = buildTopologyNodes("local-master-node", samplePeers);
    expect(nodes.length).toBe(4); // Master + Vault + 2 peers
    expect(nodes[0].category).toBe("master");
    expect(nodes[1].category).toBe("storage");
    expect(nodes[2].category).toBe("employee");
    expect(nodes[3].category).toBe("storage");
  });

  it("renders topology graph title and badges", () => {
    render(<MeshTopologyGraph localNodeId="master-001" peers={samplePeers} />);
    expect(screen.getByText("Visual Mesh Topology Graph")).toBeInTheDocument();
    expect(screen.getByText("Master Host")).toBeInTheDocument();
    expect(screen.getByText("Private Storage Vault")).toBeInTheDocument();
    expect(screen.getByText("Dev Laptop 01")).toBeInTheDocument();
    expect(screen.getByText("Storage Node East")).toBeInTheDocument();
  });

  it("opens revocation confirmation modal on Disconnect & Purge click", () => {
    render(<MeshTopologyGraph localNodeId="master-001" peers={samplePeers} />);
    const disconnectBtn = screen.getByRole("button", {
      name: /Disconnect & Purge Dev Laptop 01/i,
    });
    fireEvent.click(disconnectBtn);

    expect(
      screen.getByText("Confirm Offboarding & Node Revocation"),
    ).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText("DISCONNECT"),
    ).toBeInTheDocument();
  });

  it("executes offboard disconnect callback when confirmed with DISCONNECT", async () => {
    const handleDisconnect = vi.fn();
    render(
      <MeshTopologyGraph
        localNodeId="master-001"
        peers={samplePeers}
        onDisconnectPeer={handleDisconnect}
      />,
    );

    const disconnectBtn = screen.getByRole("button", {
      name: /Disconnect & Purge Dev Laptop 01/i,
    });
    fireEvent.click(disconnectBtn);

    const input = screen.getByPlaceholderText("DISCONNECT");
    const confirmBtn = screen.getByRole("button", {
      name: /Confirm & Revoke Node/i,
    });

    expect(confirmBtn).toBeDisabled();

    fireEvent.change(input, { target: { value: "DISCONNECT" } });
    expect(confirmBtn).not.toBeDisabled();

    fireEvent.click(confirmBtn);

    await waitFor(() => {
      expect(handleDisconnect).toHaveBeenCalledWith("peer-employee-1");
    });
  });

  it("can cancel modal without calling disconnect", () => {
    const handleDisconnect = vi.fn();
    render(
      <MeshTopologyGraph
        localNodeId="master-001"
        peers={samplePeers}
        onDisconnectPeer={handleDisconnect}
      />,
    );

    const disconnectBtn = screen.getByRole("button", {
      name: /Disconnect & Purge Dev Laptop 01/i,
    });
    fireEvent.click(disconnectBtn);

    const cancelBtn = screen.getByRole("button", { name: "Cancel" });
    fireEvent.click(cancelBtn);

    expect(
      screen.queryByText("Confirm Offboarding & Node Revocation"),
    ).not.toBeInTheDocument();
    expect(handleDisconnect).not.toHaveBeenCalled();
  });
});
