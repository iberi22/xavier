import React from "react";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { ColabAuthButton } from "../src/components/ColabAuthButton";

describe("ColabAuthButton", () => {
  const originalFetch = global.fetch;

  beforeEach(() => {
    vi.restoreAllMocks();
  });

  afterEach(() => {
    global.fetch = originalFetch;
  });

  it("renders initial disconnected state when API returns 404/not connected", async () => {
    global.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 404,
    });

    render(<ColabAuthButton />);

    await waitFor(() => {
      expect(screen.getByText("Conectar con Google")).toBeInTheDocument();
    });
  });

  it("renders connected state when API status returns connected and user email", async () => {
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (url.includes("/auth/google/status")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              connected: true,
              email: "testuser@gmail.com",
            }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<ColabAuthButton />);

    await waitFor(() => {
      expect(screen.getByText("testuser@gmail.com")).toBeInTheDocument();
      expect(screen.getByText("Desconectar")).toBeInTheDocument();
    });
  });

  it("handles connect flow by requesting /auth/google/connect and opening popup", async () => {
    const windowOpenSpy = vi.spyOn(window, "open").mockReturnValue({
      closed: true,
    } as any);

    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (url.includes("/auth/google/status")) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ connected: false }),
        });
      }
      if (url.includes("/auth/google/connect")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              auth_url: "https://accounts.google.com/o/oauth2/auth",
            }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<ColabAuthButton />);

    const connectBtn = await screen.findByText("Conectar con Google");
    fireEvent.click(connectBtn);

    await waitFor(() => {
      expect(windowOpenSpy).toHaveBeenCalledWith(
        "https://accounts.google.com/o/oauth2/auth",
        "google_oauth",
        expect.any(String)
      );
    });
  });

  it("handles disconnect flow by sending DELETE /auth/google/disconnect", async () => {
    let disconnectedCalled = false;

    global.fetch = vi.fn().mockImplementation((url: string, options?: any) => {
      if (url.includes("/auth/google/status")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              connected: true,
              email: "user@gmail.com",
            }),
        });
      }
      if (url.includes("/auth/google/disconnect") && options?.method === "DELETE") {
        disconnectedCalled = true;
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ status: "ok" }),
        });
      }
      return Promise.resolve({ ok: false });
    });

    render(<ColabAuthButton />);

    const disconnectBtn = await screen.findByText("Desconectar");
    fireEvent.click(disconnectBtn);

    await waitFor(() => {
      expect(disconnectedCalled).toBe(true);
      expect(screen.getByText("Conectar con Google")).toBeInTheDocument();
    });
  });
});
