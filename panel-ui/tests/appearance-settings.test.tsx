import { act, render, screen } from "@testing-library/react";
import React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { ThemeProvider } from "../src/lib/theme/theme-provider";
import AppearancePage from "../src/pages/Settings/Appearance";

describe("Appearance Settings Page", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.className = "";
    document.documentElement.removeAttribute("data-theme");
  });

  it("renders theme cards including Studio Dark as default and Xavier Cyberpunk as secondary", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    expect(screen.getByText("Aspecto y Temas")).toBeDefined();
    expect(screen.getByText("Studio Dark")).toBeDefined();
    expect(screen.getByText("Hueso Blanco")).toBeDefined();
    expect(screen.getByText("Xavier Cyberpunk")).toBeDefined();
    expect(screen.getByText("Predeterminado")).toBeDefined();
    expect(screen.getByText("Secundario")).toBeDefined();
  });

  it("changes theme when clicking a theme card", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    const boneCard = screen.getByText("Hueso Blanco");
    act(() => {
      boneCard.click();
    });

    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-bone");
    expect(document.documentElement.classList.contains("studio-bone")).toBe(true);
  });

  it("displays micro-interactions and typography toggles", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    const fontCheckbox = screen.getByLabelText("Usar fuente del sistema") as HTMLInputElement;
    expect(fontCheckbox.checked).toBe(true);

    act(() => {
      fontCheckbox.click();
    });

    expect(fontCheckbox.checked).toBe(false);
  });
});
