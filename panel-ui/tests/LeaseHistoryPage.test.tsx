import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiClient } from "../src/api/client";
import LeaseHistoryPage from "../src/pages/Settings/LeaseHistoryPage";
import type { SecretAuditLog } from "../src/types";

const LEND_ROW: SecretAuditLog = {
  id: 1,
  event_type: "LEND",
  timestamp: new Date().toISOString(),
  agent_id: "agent-alpha",
  secret_id: "openai-api-key",
  lease_id: "3f9a1c0de2b74a8e9d01aa55bb66cc77",
  reason: null,
};

const REVOKE_ROW: SecretAuditLog = {
  id: 2,
  event_type: "REVOKE",
  timestamp: new Date().toISOString(),
  agent_id: "agent-beta",
  secret_id: null,
  lease_id: "9b2e7d4f1a0c3e5b8d6f2a1c4e7b9d03",
  reason: "Manual API Call",
};

describe("LeaseHistoryPage", () => {
  beforeEach(() => {
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("loads audit rows and names each lease by its opaque id prefix", async () => {
    const getLeaseHistory = vi
      .spyOn(ApiClient.prototype, "getLeaseHistory")
      .mockResolvedValue([LEND_ROW, REVOKE_ROW]);

    render(<LeaseHistoryPage token="session-token" />);

    expect(await screen.findByText("Secret Lent")).toBeInTheDocument();
    expect(screen.getByText("Lease Revoked")).toBeInTheDocument();
    expect(getLeaseHistory).toHaveBeenCalledTimes(1);

    expect(screen.getByText("agent-alpha")).toBeInTheDocument();
    expect(screen.getByText("openai-api-key")).toBeInTheDocument();
    expect(screen.getByText("Manual API Call")).toBeInTheDocument();
    expect(screen.getByText("3f9a1c0d...")).toBeInTheDocument();
    expect(screen.getByText("9b2e7d4f...")).toBeInTheDocument();
  });

  it("shows the empty state when there are no audit rows", async () => {
    vi.spyOn(ApiClient.prototype, "getLeaseHistory").mockResolvedValue([]);

    render(<LeaseHistoryPage token="session-token" />);

    expect(await screen.findByText("No audit logs found.")).toBeInTheDocument();
  });

  it("shows an error when the load fails", async () => {
    vi.spyOn(ApiClient.prototype, "getLeaseHistory").mockRejectedValue(
      new Error("network down"),
    );

    render(<LeaseHistoryPage token="session-token" />);

    expect(
      await screen.findByText("Failed to fetch lease audit logs"),
    ).toBeInTheDocument();
  });

  it("refreshes the history when the refresh button is pressed", async () => {
    const getLeaseHistory = vi
      .spyOn(ApiClient.prototype, "getLeaseHistory")
      .mockResolvedValueOnce([LEND_ROW])
      .mockResolvedValueOnce([LEND_ROW, REVOKE_ROW]);

    render(<LeaseHistoryPage token="session-token" />);
    expect(await screen.findByText("Secret Lent")).toBeInTheDocument();
    expect(screen.queryByText("Lease Revoked")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Refresh History" }));

    expect(await screen.findByText("Lease Revoked")).toBeInTheDocument();
    await waitFor(() => expect(getLeaseHistory).toHaveBeenCalledTimes(2));
  });

  it("renders a revoke event as a revoked lease without any token field", async () => {
    // A row from the backend carries only `lease_id`; a legacy `session_token`
    // must not be needed for the page to render.
    vi.spyOn(ApiClient.prototype, "getLeaseHistory").mockResolvedValue([
      REVOKE_ROW,
    ]);

    render(<LeaseHistoryPage token="session-token" />);

    expect(await screen.findByText("Lease Revoked")).toBeInTheDocument();
    expect(screen.getByText("9b2e7d4f...")).toBeInTheDocument();
    expect(screen.queryByText("Secret Lent")).not.toBeInTheDocument();
  });
});
