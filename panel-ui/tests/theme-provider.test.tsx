import { render, screen, act } from "@testing-library/react";
import React from "react";
import { describe, expect, it, beforeEach } from "vitest";
import { ThemeProvider, useTheme } from "../src/lib/theme/theme-provider";
import type { ThemeMode } from "../src/lib/theme/types";

function TestConsumer() {
  const { theme, resolvedTheme, setTheme } = useTheme();

  return (
    <div>
      <span data-testid="current-theme">{theme}</span>
      <span data-testid="resolved-theme">{resolvedTheme}</span>
      <button
        type="button"
        data-testid="btn-studio-bone"
        onClick={() => setTheme("studio-bone")}
      >
        Set Studio Bone
      </button>
      <button
        type="button"
        data-testid="btn-cyberpunk"
        onClick={() => setTheme("cyberpunk")}
      >
        Set Cyberpunk
      </button>
      <button
        type="button"
        data-testid="btn-studio-dark"
        onClick={() => setTheme("studio-dark")}
      >
        Set Studio Dark
      </button>
    </div>
  );
}

describe("ThemeProvider & Multi-Theme Token Engine", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.className = "";
    document.documentElement.removeAttribute("data-theme");
    const existingMeta = document.querySelector('meta[name="theme-color"]');
    if (existingMeta) {
      existingMeta.remove();
    }
  });

  it("defaults theme to studio-dark and sets data-theme attribute", () => {
    render(
      <ThemeProvider>
        <TestConsumer />
      </ThemeProvider>,
    );

    expect(screen.getByTestId("current-theme").textContent).toBe("studio-dark");
    expect(screen.getByTestId("resolved-theme").textContent).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    const metaTheme = document.querySelector('meta[name="theme-color"]');
    expect(metaTheme).not.toBeNull();
    expect(metaTheme?.getAttribute("content")).toBe("#0d0e10");
  });

  it("updates theme to studio-bone correctly", async () => {
    render(
      <ThemeProvider>
        <TestConsumer />
      </ThemeProvider>,
    );

    act(() => {
      screen.getByTestId("btn-studio-bone").click();
    });

    expect(screen.getByTestId("current-theme").textContent).toBe("studio-bone");
    expect(screen.getByTestId("resolved-theme").textContent).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-bone");
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(document.documentElement.classList.contains("dark")).toBe(false);

    const metaTheme = document.querySelector('meta[name="theme-color"]');
    expect(metaTheme?.getAttribute("content")).toBe("#f7f6f2");
  });

  it("updates theme to cyberpunk correctly", async () => {
    render(
      <ThemeProvider>
        <TestConsumer />
      </ThemeProvider>,
    );

    act(() => {
      screen.getByTestId("btn-cyberpunk").click();
    });

    expect(screen.getByTestId("current-theme").textContent).toBe("cyberpunk");
    expect(screen.getByTestId("resolved-theme").textContent).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("cyberpunk");
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    const metaTheme = document.querySelector('meta[name="theme-color"]');
    expect(metaTheme?.getAttribute("content")).toBe("#050505");
  });
});
