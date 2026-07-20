import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { LoginPage } from "../src/auth/LoginPage";
import { useAuthStore } from "../src/auth/AuthProvider";
import React from "react";

// Mock useAuthStore
vi.mock("../src/auth/AuthProvider", () => ({
  useAuthStore: vi.fn((selector) =>
    selector({
      user: null,
      token: null,
      isAuthenticated: false,
      requires2FA: false,
      login: vi.fn(),
      logout: vi.fn(),
      register: vi.fn(),
    }),
  ),
}));

// Mock ParticleBackground
vi.mock("../src/components/ParticleBackground", () => ({
  default: () => <div data-testid="particle-bg" />,
}));

describe("LoginPage", () => {
  it("renders login form correctly", () => {
    render(<LoginPage />);

    expect(screen.getByText("XAVIER LOGIN")).toBeInTheDocument();
    expect(screen.getByLabelText("Email")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "INITIALIZE SESSION" }),
    ).toBeInTheDocument();
  });

  it("calls login function on form submit", async () => {
    const loginMock = vi.fn().mockResolvedValue(undefined);
    (useAuthStore as any).mockImplementation((selector: any) =>
      selector({
        user: null,
        token: null,
        isAuthenticated: false,
        requires2FA: false,
        login: loginMock,
      }),
    );

    render(<LoginPage />);

    fireEvent.change(screen.getByLabelText("Email"), {
      target: { value: "test@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "password123" },
    });
    fireEvent.click(screen.getByRole("button", { name: "INITIALIZE SESSION" }));

    await waitFor(() => {
      expect(loginMock).toHaveBeenCalledWith(
        "test@example.com",
        "password123",
        undefined,
      );
    });
  });

  it("shows 2FA input when requires2FA is true", () => {
    (useAuthStore as any).mockImplementation((selector: any) =>
      selector({
        user: null,
        token: null,
        isAuthenticated: false,
        requires2FA: true,
        login: vi.fn(),
      }),
    );

    render(<LoginPage />);

    expect(screen.getByText("Enter 2FA Code")).toBeInTheDocument();
  });
});
