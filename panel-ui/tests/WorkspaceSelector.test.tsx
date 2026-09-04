import { fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WorkspaceSelector, {
  getActiveWorkspaceId,
  getWorkspaceList,
} from "../src/components/WorkspaceSelector";

describe("WorkspaceSelector component", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  afterEach(() => {
    localStorage.clear();
  });

  it("returns default values when localStorage is empty", () => {
    expect(getActiveWorkspaceId()).toBe("default");
    expect(getWorkspaceList()).toEqual(["default", "swal", "personal", "work"]);
  });

  it("renders active workspace name in header button", () => {
    render(<WorkspaceSelector />);
    expect(screen.getByRole("button", { name: /Select workspace/i })).toBeInTheDocument();
    expect(screen.getByText("default")).toBeInTheDocument();
  });

  it("opens dropdown on click and displays available workspaces", () => {
    render(<WorkspaceSelector />);
    const button = screen.getByRole("button", { name: /Select workspace/i });
    fireEvent.click(button);

    expect(screen.getByText("Workspaces / Vaults")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^swal$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^personal$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^work$/i })).toBeInTheDocument();
  });

  it("switches active workspace on click and dispatches event", () => {
    const listener = vi.fn();
    window.addEventListener("xavier:workspace-changed", listener);

    render(<WorkspaceSelector />);
    fireEvent.click(screen.getByRole("button", { name: /Select workspace/i }));

    const swalBtn = screen.getByRole("button", { name: /^swal$/i });
    fireEvent.click(swalBtn);

    expect(localStorage.getItem("xavier_active_workspace")).toBe("swal");
    expect(listener).toHaveBeenCalledWith(
      expect.objectContaining({
        detail: { workspaceId: "swal" },
      }),
    );
    expect(screen.getByRole("button", { name: /Select workspace/i })).toHaveTextContent("swal");

    window.removeEventListener("xavier:workspace-changed", listener);
  });

  it("allows creating a new workspace and sets it active", () => {
    const listener = vi.fn();
    window.addEventListener("xavier:workspace-changed", listener);

    render(<WorkspaceSelector />);
    fireEvent.click(screen.getByRole("button", { name: /Select workspace/i }));

    const newWsBtn = screen.getByRole("button", { name: /New Workspace/i });
    fireEvent.click(newWsBtn);

    const input = screen.getByPlaceholderText("workspace-name");
    fireEvent.change(input, { target: { value: "project-x" } });

    const submitBtn = screen.getByRole("button", { name: /Add/i });
    fireEvent.click(submitBtn);

    expect(localStorage.getItem("xavier_active_workspace")).toBe("project-x");
    expect(JSON.parse(localStorage.getItem("xavier_workspaces") || "[]")).toContain("project-x");
    expect(listener).toHaveBeenCalledWith(
      expect.objectContaining({
        detail: { workspaceId: "project-x" },
      }),
    );

    window.removeEventListener("xavier:workspace-changed", listener);
  });
});
