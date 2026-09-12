import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import React from "react";
import { PluginsManager } from "../src/components/PluginsManager";

const mockGetPlugins = vi.fn();
const mockInstallPlugin = vi.fn();

vi.mock("../src/api/client", () => {
  return {
    ApiClient: vi.fn().mockImplementation(() => ({
      getPlugins: mockGetPlugins,
      installPlugin: mockInstallPlugin,
    })),
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
    const installButtons = screen.getAllByRole("button", {
      name: /Install with 1-Click/i,
    });
    expect(installButtons.length).toBeGreaterThan(0);

    // Click install for codegraph (first card)
    fireEvent.click(installButtons[0]);

    await waitFor(() => {
      expect(mockInstallPlugin).toHaveBeenCalledWith("codegraph");
    });

    // Verify status updated to Active (Sidecar)
    await waitFor(() => {
      expect(screen.getByText("Active (Sidecar)")).toBeInTheDocument();
    });
  });
});
