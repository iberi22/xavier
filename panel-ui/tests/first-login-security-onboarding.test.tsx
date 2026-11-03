import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import React from "react";
import { FirstLoginSecurityOnboarding } from "../src/components/Onboarding/FirstLoginSecurityOnboarding";
import { useAuthStore } from "../src/auth/AuthProvider";

vi.mock("../src/auth/TwoFactorSetup", () => ({
  TwoFactorSetup: () => <div data-testid="mock-two-factor-setup">TwoFactorSetup Component</div>,
}));

describe("FirstLoginSecurityOnboarding", () => {
  beforeEach(() => {
    localStorage.clear();
    useAuthStore.setState({
      token: "mock-jwt-token",
      isAuthenticated: true,
      user: {
        id: "swal-tenant-alpha",
        email: "founder@swal.network",
        name: "founder",
        role: "admin",
        created_at: 1000,
        updated_at: 1000,
      },
    });
  });

  it("renders welcome header and security recommendation on first login", () => {
    const onComplete = vi.fn();
    render(<FirstLoginSecurityOnboarding onComplete={onComplete} />);

    expect(screen.getByText("Bienvenido a Xavier OS")).toBeInTheDocument();
    expect(screen.getByText("Configuración Inicial de Seguridad y Operador")).toBeInTheDocument();
    expect(screen.getByText("founder@swal.network")).toBeInTheDocument();
    expect(screen.getByText("swal-tenant-alpha")).toBeInTheDocument();
    expect(screen.getByText("Gemma 2 768d (In-Process)")).toBeInTheDocument();
    expect(screen.getByText("Recomendación de Seguridad")).toBeInTheDocument();
    expect(screen.getByTestId("onboarding-setup-2fa-btn")).toBeInTheDocument();
    expect(screen.getByTestId("onboarding-skip-2fa-btn")).toBeInTheDocument();
  });

  it("completes onboarding and records completion in localStorage when skipping", () => {
    const onComplete = vi.fn();
    render(<FirstLoginSecurityOnboarding onComplete={onComplete} />);

    const skipBtn = screen.getByTestId("onboarding-skip-2fa-btn");
    fireEvent.click(skipBtn);

    expect(localStorage.getItem("xavier_security_onboarding_completed")).toBe("true");
    expect(onComplete).toHaveBeenCalledTimes(1);
  });

  it("transitions to 2FA setup when clicking 'Configurar 2FA Ahora'", () => {
    const onComplete = vi.fn();
    render(<FirstLoginSecurityOnboarding onComplete={onComplete} />);

    const setupBtn = screen.getByTestId("onboarding-setup-2fa-btn");
    fireEvent.click(setupBtn);

    expect(screen.getByTestId("mock-two-factor-setup")).toBeInTheDocument();
    expect(screen.getByTestId("onboarding-abort-2fa-btn")).toBeInTheDocument();

    // Clicking abort from within 2FA setup marks onboarding completed and returns
    fireEvent.click(screen.getByTestId("onboarding-abort-2fa-btn"));
    expect(localStorage.getItem("xavier_security_onboarding_completed")).toBe("true");
    expect(onComplete).toHaveBeenCalledTimes(1);
  });
});
