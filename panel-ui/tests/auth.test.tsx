import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { LoginPage } from "../src/auth/LoginPage";
import React from "react";

// Mock ParticleBackground
vi.mock("../src/components/ParticleBackground", () => ({
  default: () => <div data-testid="particle-bg" />,
}));

describe("LoginPage (Passwordless Magic Link)", () => {
  const originalFetch = global.fetch;

  beforeEach(() => {
    vi.restoreAllMocks();
  });

  afterEach(() => {
    global.fetch = originalFetch;
  });

  it("renders email-only passwordless form correctly", () => {
    render(<LoginPage />);

    expect(screen.getByText("XAVIER LOGIN")).toBeInTheDocument();
    expect(
      screen.getByText("Connect to the secure cognitive core"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Email")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "INITIALIZE SESSION" }),
    ).toBeInTheDocument();

    // Verify password and 2FA fields are strictly absent
    expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
    expect(screen.queryByText("Enter 2FA Code")).not.toBeInTheDocument();
    expect(screen.queryByText("REGISTER NEW OPERATOR")).not.toBeInTheDocument();
  });

  it("submits email to magic-link endpoint and shows redirect status on success", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        ok: true,
        magic_url: "https://xavier-cloud.swal.network/auth/magic?token=mtk_test123",
      }),
    });
    global.fetch = fetchMock;

    render(<LoginPage />);

    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "operator@xavier.local" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "INITIALIZE SESSION" }),
    );

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        "https://xaviercloud.swal.network/auth/magic-link",
        expect.objectContaining({
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ email: "operator@xavier.local" }),
        }),
      );
    });

    await waitFor(() => {
      expect(
        screen.getByText("MAGIC LINK GENERATED - REDIRECTING..."),
      ).toBeInTheDocument();
    });
  });

  it("displays error message when magic-link endpoint fails", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      json: async () => ({
        ok: false,
        error: "Rate limit exceeded. Please wait.",
      }),
    });
    global.fetch = fetchMock;

    render(<LoginPage />);

    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "operator@xavier.local" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "INITIALIZE SESSION" }),
    );

    await waitFor(() => {
      expect(
        screen.getByText("Rate limit exceeded. Please wait."),
      ).toBeInTheDocument();
    });
  });
});
