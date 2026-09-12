import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ConfigModal, { AppearanceSettings } from "../src/components/ConfigModal";
import { ThemeProvider } from "../src/lib/theme/theme-provider";
import type { BookmarkArtifact, GraphData } from "../src/types";

const mockGraphData: GraphData = {
  nodes: [],
  links: [],
};

const mockBookmarks: BookmarkArtifact[] = [];

describe("Appearance Settings & Micro-interactions", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.className = "";
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-neon-tokens");
    vi.restoreAllMocks();
  });

  it("renders Appearance settings standalone or within ThemeProvider", () => {
    render(
      <ThemeProvider>
        <AppearanceSettings />
      </ThemeProvider>
    );

    expect(screen.getByText("Appearance & System Redesign")).toBeInTheDocument();
    expect(screen.getByText("Theme Presets")).toBeInTheDocument();
    expect(screen.getByText("Micro-Interactions")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Select Studio Dark theme" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Select Studio Bone theme" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Select Cyberpunk theme" })).toBeInTheDocument();
  });

  it("renders Appearance tab inside ConfigModal when navigated to", async () => {
    render(
      <ThemeProvider>
        <ConfigModal
          onClose={vi.fn()}
          graphData={mockGraphData}
          onUpdateGraphData={vi.fn()}
          bookmarks={mockBookmarks}
          onPinArtifact={vi.fn()}
          onUpdateBookmark={vi.fn()}
        />
      </ThemeProvider>
    );

    const appearanceTabBtn = screen.getByRole("button", { name: "Appearance" });
    expect(appearanceTabBtn).toBeInTheDocument();

    act(() => {
      fireEvent.click(appearanceTabBtn);
    });

    await waitFor(() => {
      expect(screen.getByText("Appearance & System Redesign")).toBeInTheDocument();
    });
  });

  it("handles theme card clicks to switch active theme", () => {
    render(
      <ThemeProvider>
        <AppearanceSettings />
      </ThemeProvider>
    );

    const boneCard = screen.getByRole("radio", { name: "Select Studio Bone theme" });
    expect(boneCard).toBeInTheDocument();

    act(() => {
      fireEvent.click(boneCard);
    });

    expect(localStorage.getItem("vite-ui-theme")).toBe("studio-bone");
    expect(document.documentElement.getAttribute("data-theme")).toBe("studio-bone");
    expect(document.documentElement.classList.contains("studio-bone")).toBe(true);

    const cyberpunkCard = screen.getByRole("radio", { name: "Select Cyberpunk theme" });

    act(() => {
      fireEvent.click(cyberpunkCard);
    });

    expect(localStorage.getItem("vite-ui-theme")).toBe("cyberpunk");
    expect(document.documentElement.getAttribute("data-theme")).toBe("cyberpunk");
    expect(document.documentElement.getAttribute("data-neon-tokens")).toBe("true");
  });

  it("handles micro-interaction toggles (icon borders and attenuation)", () => {
    render(
      <ThemeProvider>
        <AppearanceSettings />
      </ThemeProvider>
    );

    const iconBordersToggle = screen.getByRole("switch", { name: "Toggle icon borders" });
    expect(iconBordersToggle).toHaveAttribute("aria-checked", "true");

    act(() => {
      fireEvent.click(iconBordersToggle);
    });

    expect(iconBordersToggle).toHaveAttribute("aria-checked", "false");

    const attenuationToggle = screen.getByRole("switch", { name: "Toggle attenuation" });
    expect(attenuationToggle).toHaveAttribute("aria-checked", "false");

    act(() => {
      fireEvent.click(attenuationToggle);
    });

    expect(attenuationToggle).toHaveAttribute("aria-checked", "true");
  });
});
