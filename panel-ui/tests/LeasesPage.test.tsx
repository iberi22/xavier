import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiClient } from "../src/api/client";
import LeasesPage from "../src/pages/Settings/LeasesPage";
import type { SecretLease } from "../src/types";

const FUTURE = new Date(Date.now() + 60 * 60 * 1000).toISOString();

// Opaque lease ids as the backend emits them (`lease_id_for_token`), never tokens.
const LEASE_A: SecretLease = {
  lease_id: "3f9a1c0de2b74a8e9d01aa55bb66cc77",
  secret_name: "openai-api-key",
  agent_id: "agent-alpha",
  created_at: new Date().toISOString(),
  expires_at: FUTURE,
};

const LEASE_B: SecretLease = {
  lease_id: "9b2e7d4f1a0c3e5b8d6f2a1c4e7b9d03",
  secret_name: "github-token",
  agent_id: "agent-beta",
  created_at: new Date().toISOString(),
  expires_at: FUTURE,
};

describe("LeasesPage", () => {
  beforeEach(() => {
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("loads leases by opaque lease id and shows only the id prefix", async () => {
    const getLeases = vi
      .spyOn(ApiClient.prototype, "getLeases")
      .mockResolvedValue([LEASE_A, LEASE_B]);

    render(<LeasesPage token="session-token" />);

    expect(await screen.findByText("openai-api-key")).toBeInTheDocument();
    expect(screen.getByText("github-token")).toBeInTheDocument();
    expect(getLeases).toHaveBeenCalledTimes(1);

    expect(screen.getByText("3f9a1c0d...")).toBeInTheDocument();
    expect(screen.getByText("9b2e7d4f...")).toBeInTheDocument();
    expect(screen.getByTitle(LEASE_A.lease_id)).toBeInTheDocument();
  });

  it("shows the empty state when there are no leases", async () => {
    vi.spyOn(ApiClient.prototype, "getLeases").mockResolvedValue([]);

    render(<LeasesPage token="session-token" />);

    expect(
      await screen.findByText("No active secret leases found."),
    ).toBeInTheDocument();
  });

  it("shows an error when the load fails", async () => {
    vi.spyOn(ApiClient.prototype, "getLeases").mockRejectedValue(
      new Error("network down"),
    );

    render(<LeasesPage token="session-token" />);

    expect(
      await screen.findByText("Failed to fetch active leases"),
    ).toBeInTheDocument();
  });

  it("refreshes the list when the refresh button is pressed", async () => {
    const getLeases = vi
      .spyOn(ApiClient.prototype, "getLeases")
      .mockResolvedValueOnce([LEASE_A])
      .mockResolvedValueOnce([LEASE_A, LEASE_B]);

    render(<LeasesPage token="session-token" />);
    expect(await screen.findByText("openai-api-key")).toBeInTheDocument();
    expect(screen.queryByText("github-token")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTitle("Refresh Leases"));

    expect(await screen.findByText("github-token")).toBeInTheDocument();
    expect(getLeases).toHaveBeenCalledTimes(2);
  });

  it("revokes by lease id and drops only that lease from the list", async () => {
    vi.spyOn(ApiClient.prototype, "getLeases").mockResolvedValue([
      LEASE_A,
      LEASE_B,
    ]);
    const revokeLease = vi
      .spyOn(ApiClient.prototype, "revokeLease")
      .mockResolvedValue({ status: "revoked" });

    render(<LeasesPage token="session-token" />);
    expect(await screen.findByText("openai-api-key")).toBeInTheDocument();

    fireEvent.click(screen.getAllByTitle("Revoke Lease")[0]);

    await waitFor(() =>
      expect(screen.queryByText("openai-api-key")).not.toBeInTheDocument(),
    );
    expect(revokeLease).toHaveBeenCalledWith(LEASE_A.lease_id);
    expect(screen.getByText("github-token")).toBeInTheDocument();
  });

  it("keeps the lease listed when revoke fails", async () => {
    vi.spyOn(ApiClient.prototype, "getLeases").mockResolvedValue([LEASE_A]);
    vi.spyOn(ApiClient.prototype, "revokeLease").mockRejectedValue(
      new Error("revoke failed"),
    );

    render(<LeasesPage token="session-token" />);
    expect(await screen.findByText("openai-api-key")).toBeInTheDocument();

    fireEvent.click(screen.getByTitle("Revoke Lease"));

    await waitFor(() => expect(console.error).toHaveBeenCalled());
    expect(screen.getByText("openai-api-key")).toBeInTheDocument();
  });
});
