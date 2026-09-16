import { act, fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { ThemeProvider } from "../src/lib/theme/theme-provider";
import AppearancePage from "../src/pages/Settings/Appearance";

describe("Appearance Presets & Custom Color Customization", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.className = "";
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.style.cssText = "";
  });

  it("renders custom color swatch pickers and badges", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    expect(screen.getByText("Personalización de Paleta de Colores")).toBeDefined();
    expect(screen.getByText("Color de Fondo")).toBeDefined();
    expect(screen.getByText("Color Texto / Texto Principal")).toBeDefined();
    expect(screen.getByText("Color de Acento")).toBeDefined();
  });

  it("updates custom dark colors and applies CSS variables and persists to localStorage", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    const bgInput = screen.getByLabelText("Color de fondo input") as HTMLInputElement;

    act(() => {
      fireEvent.change(bgInput, { target: { value: "#121212" } });
    });

    expect(document.documentElement.style.getPropertyValue("--background")).toBe("#121212");

    const savedSettings = JSON.parse(localStorage.getItem("test-theme-settings") || "{}");
    expect(savedSettings.customDarkColors?.background).toBe("#121212");
  });

  it("updates custom light colors and applies CSS variables when in light theme", () => {
    render(
      <ThemeProvider defaultTheme="studio-bone" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    const accentInput = screen.getByLabelText("Color de acento input") as HTMLInputElement;

    act(() => {
      fireEvent.change(accentInput, { target: { value: "#ff0055" } });
    });

    expect(document.documentElement.style.getPropertyValue("--accent")).toBe("#ff0055");

    const savedSettings = JSON.parse(localStorage.getItem("test-theme-settings") || "{}");
    expect(savedSettings.customLightColors?.accent).toBe("#ff0055");
  });

  it("resets custom color overrides when reset button is clicked", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    const bgInput = screen.getByLabelText("Color de fondo input") as HTMLInputElement;

    act(() => {
      fireEvent.change(bgInput, { target: { value: "#1a1a1a" } });
    });

    const resetButton = screen.getByText("Restablecer Valores");
    expect(resetButton).toBeDefined();

    act(() => {
      resetButton.click();
    });

    const savedSettings = JSON.parse(localStorage.getItem("test-theme-settings") || "{}");
    expect(savedSettings.customDarkColors).toBeUndefined();
  });
});
