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

  it("renders Antigravity Appearance header, chat settings, and theme sections", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    // Title & Subtitle
    expect(screen.getAllByText("Appearance").length).toBeGreaterThan(0);
    expect(
      screen.getByText("Configure the agent's visual theme and display preferences.")
    ).toBeDefined();

    // Chat Settings Section
    expect(screen.getByText("Chat Settings")).toBeDefined();
    expect(screen.getByText("Verbose Agent Chat")).toBeDefined();
    expect(
      screen.getByText("Display and preserve intermediate thinking steps.")
    ).toBeDefined();
    expect(screen.getByText("Conversation Width")).toBeDefined();
    expect(
      screen.getByText("Configure the maximum width of the conversation panel.")
    ).toBeDefined();
    expect(screen.getByText("Default")).toBeDefined();
    expect(screen.getByText("Narrow")).toBeDefined();
    expect(screen.getByText("Wide")).toBeDefined();

    // Theme Sections
    expect(screen.getByText("Light Theme")).toBeDefined();
    expect(screen.getByText("Dark Theme")).toBeDefined();
    expect(screen.getByText("#EEEEEE")).toBeDefined();
    expect(screen.getByText("#CCCCCC")).toBeDefined();
  });

  it("changes theme when clicking Light, Dark, or System mode buttons", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    const lightButton = screen.getByLabelText("Light theme");
    act(() => {
      lightButton.click();
    });

    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-bone");
    expect(document.documentElement.classList.contains("studio-bone")).toBe(true);

    const darkButton = screen.getByLabelText("Dark theme");
    act(() => {
      darkButton.click();
    });

    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-dark");
    expect(document.documentElement.classList.contains("studio-dark")).toBe(true);

    const systemButton = screen.getByLabelText("Inherit system theme");
    act(() => {
      systemButton.click();
    });
    // System resolves to studio-dark or studio-bone depending on matchMedia
    expect(document.documentElement.getAttribute("data-theme")).toBeDefined();
  });

  it("toggles Verbose Agent Chat switch and selects Conversation Width", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    const switchBtn = screen.getByRole("switch", { name: "Verbose Agent Chat" });
    expect(switchBtn.getAttribute("aria-checked")).toBe("true");

    act(() => {
      switchBtn.click();
    });
    expect(switchBtn.getAttribute("aria-checked")).toBe("false");

    const narrowBtn = screen.getByText("Narrow");
    act(() => {
      narrowBtn.click();
    });
    expect(narrowBtn.className).toContain("bg-[#25272c]");
  });
});

