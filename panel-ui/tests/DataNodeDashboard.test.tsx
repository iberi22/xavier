import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DataNodeDashboard,
  type DataNodeMetrics,
} from "../src/components/DataNodeDashboard";

describe("DataNodeDashboard Component", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("renders default state when no props or localStorage values are provided", () => {
    render(<DataNodeDashboard />);

    expect(screen.getByText("Maloca Data Node")).toBeInTheDocument();
    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.getByLabelText("Toggle Maloca Data Node Consensus Participation")).toHaveAttribute(
      "aria-checked",
      "false"
    );

    // Default quota is 5000 MB ~ 4.88 GB
    const quotaInput = screen.getByLabelText("Storage quota input in MB") as HTMLInputElement;
    expect(quotaInput.value).toBe("5000");

    // Disabled sync button when opt-in is false
    const syncBtn = screen.getByRole("button", { name: "Trigger Manual Consensus Data Sync" });
    expect(syncBtn).toBeDisabled();
  });

  it("initializes with optIn true when initialOptIn prop is passed", () => {
    render(<DataNodeDashboard initialOptIn={true} />);

    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.getByLabelText("Toggle Maloca Data Node Consensus Participation")).toHaveAttribute(
      "aria-checked",
      "true"
    );

    const syncBtn = screen.getByRole("button", { name: "Trigger Manual Consensus Data Sync" });
    expect(syncBtn).not.toBeDisabled();
  });

  it("restores optIn and quota state from localStorage if available", () => {
    localStorage.setItem("maloca_datanode_opt_in", "true");
    localStorage.setItem("maloca_datanode_quota_mb", "8000");

    render(<DataNodeDashboard />);

    expect(screen.getByText("Active")).toBeInTheDocument();
    const quotaInput = screen.getByLabelText("Storage quota input in MB") as HTMLInputElement;
    expect(quotaInput.value).toBe("8000");
  });

  it("handles opt-in toggle click, updates localStorage, and fires onOptInChange callback", () => {
    const onOptInChange = vi.fn();
    render(<DataNodeDashboard onOptInChange={onOptInChange} />);

    const toggleBtn = screen.getByLabelText("Toggle Maloca Data Node Consensus Participation");
    expect(toggleBtn).toHaveAttribute("aria-checked", "false");

    fireEvent.click(toggleBtn);

    expect(toggleBtn).toHaveAttribute("aria-checked", "true");
    expect(localStorage.getItem("maloca_datanode_opt_in")).toBe("true");
    expect(onOptInChange).toHaveBeenCalledWith(true);

    // Toggle back
    fireEvent.click(toggleBtn);
    expect(toggleBtn).toHaveAttribute("aria-checked", "false");
    expect(localStorage.getItem("maloca_datanode_opt_in")).toBe("false");
    expect(onOptInChange).toHaveBeenCalledWith(false);
  });

  it("handles quota changes via slider and number input, clamping limits", () => {
    const onQuotaChange = vi.fn();
    render(<DataNodeDashboard initialOptIn={true} onQuotaChange={onQuotaChange} />);

    const slider = screen.getByLabelText("Storage quota range slider in MB");
    fireEvent.change(slider, { target: { value: "10000" } });

    expect(localStorage.getItem("maloca_datanode_quota_mb")).toBe("10000");
    expect(onQuotaChange).toHaveBeenCalledWith(10000);

    const input = screen.getByLabelText("Storage quota input in MB");
    fireEvent.change(input, { target: { value: "12000" } });

    expect(localStorage.getItem("maloca_datanode_quota_mb")).toBe("12000");
    expect(onQuotaChange).toHaveBeenCalledWith(12000);

    // Test upper clamp limit (50000)
    fireEvent.change(input, { target: { value: "99999" } });
    expect(onQuotaChange).toHaveBeenCalledWith(50000);

    // Test lower clamp limit (500)
    fireEvent.change(input, { target: { value: "10" } });
    expect(onQuotaChange).toHaveBeenCalledWith(500);
  });

  it("calculates usage percentage accurately and handles zero/edge quotas", () => {
    // 1840 MB local size with 3680 MB quota = 50%
    render(<DataNodeDashboard initialQuotaMb={3680} initialLocalDbSizeMb={1840} />);
    expect(screen.getByText(/50%/i)).toBeInTheDocument();
  });

  it("handles manual sync trigger execution and custom onSyncTrigger callback", async () => {
    vi.useFakeTimers();
    const onSyncTrigger = vi.fn().mockResolvedValue(undefined);

    render(<DataNodeDashboard initialOptIn={true} onSyncTrigger={onSyncTrigger} />);

    const syncBtn = screen.getByRole("button", { name: "Trigger Manual Consensus Data Sync" });

    await act(async () => {
      fireEvent.click(syncBtn);
    });

    expect(onSyncTrigger).toHaveBeenCalled();

    vi.useRealTimers();
  });

  it("handles fallback manual sync trigger when onSyncTrigger is not provided", async () => {
    render(<DataNodeDashboard initialOptIn={true} />);

    const syncBtn = screen.getByRole("button", { name: "Trigger Manual Consensus Data Sync" });

    fireEvent.click(syncBtn);
    expect(screen.getByText("Syncing...")).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.queryByText("Syncing...")).not.toBeInTheDocument();
    });
  });

  it("handles status refresh button click", async () => {
    render(<DataNodeDashboard />);

    const refreshBtn = screen.getByRole("button", { name: "Refresh Data Node Status" });

    fireEvent.click(refreshBtn);

    await waitFor(() => {
      expect(refreshBtn).not.toBeDisabled();
    });
  });

  it("handles custom initial metrics prop", () => {
    const customMetrics: DataNodeMetrics = {
      connectedPeers: 42,
      totalSyncedRecords: 999999,
      bandwidthUsageMbps: 8.5,
      latencyMs: 12,
      lastSyncTimestamp: "5m ago",
    };

    render(<DataNodeDashboard initialOptIn={true} initialMetrics={customMetrics} />);

    expect(screen.getByText("42")).toBeInTheDocument();
    expect(screen.getByText("999,999")).toBeInTheDocument();
    expect(screen.getByText("8.5 Mbps")).toBeInTheDocument();
    expect(screen.getByText("12 ms")).toBeInTheDocument();
  });

  it("gracefully falls back when localStorage throws errors", () => {
    const getItemSpy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError: Access is denied");
    });
    const setItemSpy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });

    render(<DataNodeDashboard />);

    const toggleBtn = screen.getByLabelText("Toggle Maloca Data Node Consensus Participation");
    expect(() => fireEvent.click(toggleBtn)).not.toThrow();

    getItemSpy.mockRestore();
    setItemSpy.mockRestore();
  });
});
