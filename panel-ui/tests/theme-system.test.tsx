import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider, useTheme } from "../src/lib/theme/theme-provider";

const TestThemeComponent = () => {
  const { theme, setTheme } = useTheme();

  return (
    <div>
      <span data-testid="current-theme">{theme}</span>
      <button onClick={() => setTheme("studio-dark")}>Set Studio Dark</button>
      <button onClick={() => setTheme("studio-bone")}>Set Studio Bone</button>
      <button onClick={() => setTheme("cyberpunk")}>Set Cyberpunk</button>
      <button onClick={() => setTheme("system")}>Set System</button>
    </div>
  );
};

describe("Theme System & ThemeProvider", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.className = "";
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-neon-tokens");
    vi.restoreAllMocks();
  });

  it("initializes default theme as studio-dark", () => {
    render(
      <ThemeProvider>
        <TestThemeComponent />
      </ThemeProvider>
    );

    expect(screen.getByTestId("current-theme")).toHaveTextContent("studio-dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-dark");
    expect(document.documentElement.classList.contains("studio-dark")).toBe(true);
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });

  it("switches theme and updates localStorage and document.documentElement attributes", () => {
    render(
      <ThemeProvider>
        <TestThemeComponent />
      </ThemeProvider>
    );

    const boneButton = screen.getByText("Set Studio Bone");
    act(() => {
      fireEvent.click(boneButton);
    });

    expect(screen.getByTestId("current-theme")).toHaveTextContent("studio-bone");
    expect(localStorage.getItem("vite-ui-theme")).toBe("studio-bone");
    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-bone");
  });

  it("switching to studio-bone updates attributes and light mode class", () => {
    render(
      <ThemeProvider>
        <TestThemeComponent />
      </ThemeProvider>
    );

    const boneButton = screen.getByText("Set Studio Bone");
    act(() => {
      fireEvent.click(boneButton);
    });

    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-bone");
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(document.documentElement.classList.contains("studio-bone")).toBe(true);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("switching to cyberpunk activates neon legacy tokens", () => {
    render(
      <ThemeProvider>
        <TestThemeComponent />
      </ThemeProvider>
    );

    const cyberpunkButton = screen.getByText("Set Cyberpunk");
    act(() => {
      fireEvent.click(cyberpunkButton);
    });

    expect(document.documentElement.getAttribute("data-theme")).toBe("cyberpunk");
    expect(document.documentElement.getAttribute("data-neon-tokens")).toBe("true");
    expect(document.documentElement.classList.contains("cyberpunk")).toBe(true);
    expect(document.documentElement.classList.contains("neon-legacy-tokens")).toBe(true);
  });

  it("handles system preference theme via matchMedia when theme is system", () => {
    const matchMediaMock = vi.fn().mockImplementation((query) => ({
      matches: query.includes("dark"),
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }));
    window.matchMedia = matchMediaMock;

    render(
      <ThemeProvider>
        <TestThemeComponent />
      </ThemeProvider>
    );

    const systemButton = screen.getByText("Set System");
    act(() => {
      fireEvent.click(systemButton);
    });

    expect(document.documentElement.getAttribute("data-theme")).toBe("system");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });

  it("throws error if useTheme is used outside ThemeProvider", () => {
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => renderHook(() => useTheme())).toThrow(
      "useTheme must be used within a ThemeProvider"
    );
    consoleErrorSpy.mockRestore();
  });
});
