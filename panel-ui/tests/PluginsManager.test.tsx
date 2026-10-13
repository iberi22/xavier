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

  it("shows an explicit unavailable state when the API returns 404", async () => {
    mockGetPlugins.mockRejectedValue(new Error("HTTP 404"));

    render(<PluginsManager token="mock-token" />);

    await waitFor(() => {
      expect(
        screen.getByText("Plugin ecosystem is not available in this build.")
      ).toBeInTheDocument();
    });

    // Ensure the default plugins are NOT rendered
    expect(screen.queryByText("codegraph")).not.toBeInTheDocument();
    expect(screen.queryByText("rtk-kernel")).not.toBeInTheDocument();
  });

  it("shows an explicit unavailable state when plugin installation returns 404", async () => {
    mockGetPlugins.mockResolvedValue([]);
    mockInstallPlugin.mockRejectedValue(new Error("HTTP 404"));

    render(<PluginsManager token="mock-token" />);

    await waitFor(() => {
      expect(screen.getByText("codegraph")).toBeInTheDocument();
    });

    const codegraphCard = screen.getByText("codegraph").closest("div.p-5")!;
    const installBtn = codegraphCard.querySelector("button")!;
    fireEvent.click(installBtn);

    await waitFor(() => {
      expect(mockInstallPlugin).toHaveBeenCalledWith("codegraph");
    });

    await waitFor(() => {
      expect(
        screen.getByText("Plugin installation is not available in this build.")
      ).toBeInTheDocument();
    });
  });
});
