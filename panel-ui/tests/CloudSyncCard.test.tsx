import { render, screen, fireEvent, act, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import CloudSyncCard from "../src/components/CloudSyncCard";
import Overview from "../src/pages/Overview";

describe("CloudSyncCard Component", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockImplementation((url: string) => {
        if (url.includes("/v1/auth/status")) {
          return Promise.resolve({
            ok: true,
            json: () =>
              Promise.resolve({
                status: "authenticated",
                user_id: "swal-tenant-alpha",
                email: "founder@swal.network",
                role: "founder",
                plan: "founder",
              }),
          });
        }
        if (url.includes("/api/v1/memory/sync/status")) {
          return Promise.resolve({
            ok: true,
            json: () =>
              Promise.resolve({
                status: "ok",
                initialized: true,
                node_id: "swal-tenant-alpha",
                sync_interval_secs: 300,
                last_session: {
                  pulled_count: 50,
                  ended_at: "2026-05-10T12:00:00Z",
                },
              }),
          });
        }
        if (url.includes("/api/v1/memory/sync/push") || url.includes("/xavier/sync/check")) {
          return Promise.resolve({
            ok: true,
            json: () => Promise.resolve({ status: "ok", pushed: 1 }),
          });
        }
        if (url.includes("/auth/magic-link")) {
          return Promise.resolve({
            ok: true,
            json: () =>
              Promise.resolve({
                ok: true,
                magic_url: "https://xavier-cloud.swal.network/auth/magic?token=xyz",
              }),
          });
        }
        return Promise.resolve({ ok: false, text: () => Promise.resolve("not found") });
      }),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("renders live metrics including plan badge, tenant, quota meter and chunks verified", async () => {
    await act(async () => {
      render(<CloudSyncCard token="test-token" />);
    });

    expect(screen.getByText("Cloudflare R2 Cloud Sync")).toBeInTheDocument();
    expect(screen.getByTestId("plan-badge")).toHaveTextContent(/founder/i);
    expect(screen.getByText("swal-tenant-alpha")).toBeInTheDocument();
    expect(screen.getByTestId("chunks-verified")).toBeInTheDocument();
    expect(screen.getByTestId("storage-meter")).toBeInTheDocument();
    expect(screen.getByTestId("storage-percentage")).toBeInTheDocument();
    expect(screen.getByTestId("last-sync-time")).toBeInTheDocument();
  });

  it("triggers incremental sync when Sync Now is clicked", async () => {
    const handleSyncComplete = vi.fn();
    await act(async () => {
      render(<CloudSyncCard token="test-token" onSyncComplete={handleSyncComplete} />);
    });

    const syncButton = screen.getByTestId("sync-now-button");
    expect(syncButton).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(syncButton);
    });

    expect(
      screen.getByText(/Incremental sync completed successfully!/i),
    ).toBeInTheDocument();
    expect(handleSyncComplete).toHaveBeenCalledTimes(1);
  });

  it("opens 1-click magic link login modal, submits email and displays confirmation", async () => {
    await act(async () => {
      render(<CloudSyncCard token="test-token" />);
    });

    const openModalBtn = screen.getByTestId("login-magic-link-button");
    expect(openModalBtn).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(openModalBtn);
    });

    expect(screen.getByText("1-Click Cloud Login")).toBeInTheDocument();
    const emailInput = screen.getByLabelText(/Email Address/i);
    expect(emailInput).toBeInTheDocument();

    await act(async () => {
      fireEvent.change(emailInput, { target: { value: "operator@swal.network" } });
    });

    const submitBtn = screen.getByText("Send Magic Link");
    await act(async () => {
      fireEvent.click(submitBtn);
    });

    await waitFor(() => {
      expect(screen.getByText(/Magic link ready!/i)).toBeInTheDocument();
    });

    // Close modal via close button
    const closeBtn = screen.getByLabelText("Close modal");
    await act(async () => {
      fireEvent.click(closeBtn);
    });

    expect(screen.queryByText("1-Click Cloud Login")).not.toBeInTheDocument();
  });

  it("renders properly inside the Overview page", async () => {
    await act(async () => {
      render(<Overview token="test-token" />);
    });

    expect(screen.getByTestId("overview-page")).toBeInTheDocument();
    expect(screen.getByText("Node Overview & Cloud Relay")).toBeInTheDocument();
    expect(screen.getByTestId("cloud-sync-card")).toBeInTheDocument();
  });
});
