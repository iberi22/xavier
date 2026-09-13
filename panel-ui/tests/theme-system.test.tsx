import { act, render, screen } from "@testing-library/react";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider, useTheme } from "../src/lib/theme/theme-provider";

function TestConsumer() {
  const { theme, resolvedTheme, setTheme, settings, updateSettings } = useTheme();
  return (
    <div>
      <span data-testid="theme">{theme}</span>
      <span data-testid="resolvedTheme">{resolvedTheme}</span>
      <span data-testid="fontSetting">{settings.useSystemFont ? "system" : "custom"}</span>
      <button onClick={() => setTheme("studio-bone")} data-testid="btn-bone">
        Set Bone
      </button>
      <button onClick={() => setTheme("cyberpunk")} data-testid="btn-cyberpunk">
        Set Cyberpunk
      </button>
      <button onClick={() => setTheme("studio-dark")} data-testid="btn-dark">
        Set Studio Dark
      </button>
      <button onClick={() => updateSettings({ useSystemFont: false })} data-testid="btn-toggle-font">
        Toggle Font
      </button>
    </div>
  );
}

describe("Theme System & ThemeProvider", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.className = "";
    document.documentElement.removeAttribute("data-theme");
  });

  it("initializes with studio-dark as default theme", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <TestConsumer />
      </ThemeProvider>
    );

    expect(screen.getByTestId("theme").textContent).toBe("studio-dark");
    expect(screen.getByTestId("resolvedTheme").textContent).toBe("studio-dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(document.documentElement.classList.contains("studio-dark")).toBe(true);
  });

  it("switches to studio-bone (Bone White) and updates DOM classes & localStorage", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <TestConsumer />
      </ThemeProvider>
    );

    act(() => {
      screen.getByTestId("btn-bone").click();
    });

    expect(screen.getByTestId("theme").textContent).toBe("studio-bone");
    expect(screen.getByTestId("resolvedTheme").textContent).toBe("studio-bone");
    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-bone");
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(document.documentElement.classList.contains("studio-bone")).toBe(true);
    expect(localStorage.getItem("test-theme")).toBe("studio-bone");
  });

  it("switches to cyberpunk secondary theme", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <TestConsumer />
      </ThemeProvider>
    );

    act(() => {
      screen.getByTestId("btn-cyberpunk").click();
    });

    expect(screen.getByTestId("theme").textContent).toBe("cyberpunk");
    expect(screen.getByTestId("resolvedTheme").textContent).toBe("cyberpunk");
    expect(document.documentElement.getAttribute("data-theme")).toBe("cyberpunk");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(document.documentElement.classList.contains("cyberpunk")).toBe(true);
    expect(localStorage.getItem("test-theme")).toBe("cyberpunk");
  });

  it("applies ergonomics settings classes (font-system, icons-bordered, attenuation-enabled)", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <TestConsumer />
      </ThemeProvider>
    );

    expect(document.documentElement.classList.contains("font-system")).toBe(true);
    expect(document.documentElement.classList.contains("icons-bordered")).toBe(true);
    expect(document.documentElement.classList.contains("attenuation-enabled")).toBe(true);

    act(() => {
      screen.getByTestId("btn-toggle-font").click();
    });

    expect(screen.getByTestId("fontSetting").textContent).toBe("custom");
    expect(document.documentElement.classList.contains("font-system")).toBe(false);
  });
});
