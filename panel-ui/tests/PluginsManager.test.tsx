import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import React from "react";
import { PluginsManager } from "../src/components/PluginsManager";

const mockGetPlugins = vi.fn();
const mockInstallPlugin = vi.fn();

vi.mock("../src/api/client", () => {
  return {
    ApiClient: class {
      getPlugins = mockGetPlugins;
      installPlugin = mockInstallPlugin;
    },
  };
});

describe("PluginsManager", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders CodeGraph as a featured plugin with description", async () => {
    mockGetPlugins.mockResolvedValue([]);

    render(<PluginsManager token="mock-token" />);

    await waitFor(() => {
      expect(screen.getByText("codegraph")).toBeInTheDocument();
    });

    expect(
      screen.getByText(
        "Colby McHenry CodeGraph Engine — Fast Tree-sitter symbol graph & AST indexing"
      )
    ).toBeInTheDocument();

    expect(screen.getByText("rtk-kernel")).toBeInTheDocument();
  });

  it("shows 'Not Installed' initially and updates to 'Active (Sidecar)' when installed", async () => {
    mockGetPlugins.mockResolvedValue([]);
    mockInstallPlugin.mockResolvedValue({ status: "ok" });

    render(<PluginsManager token="mock-token" />);

    await waitFor(() => {
      expect(screen.getByText("codegraph")).toBeInTheDocument();
    });

    // Check status pill initially shows Not Installed
    expect(screen.getAllByText("Not Installed").length).toBeGreaterThan(0);

    // Find install buttons
    // Click install for codegraph specifically
    const codegraphCard = screen.getByText("codegraph").closest("div.p-5")!;
    const installBtn = codegraphCard.querySelector("button")!;
    fireEvent.click(installBtn);

    await waitFor(() => {
      expect(mockInstallPlugin).toHaveBeenCalledWith("codegraph");
    });

    // Verify status updated to Active (Sidecar)
    await waitFor(() => {
      expect(screen.getAllByText("Active (Sidecar)").length).toBeGreaterThan(0);
    });
  });
});
