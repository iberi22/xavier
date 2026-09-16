import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ConfigModal from "../src/components/ConfigModal";

const dummyGraphData = {
  nodes: [],
  links: [],
};

describe("ConfigModal Projects Sidebar", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem(
      "xavier_workspaces",
      JSON.stringify(["default", "swal", "personal", "work"]),
    );
    localStorage.setItem("xavier_active_workspace", "default");
  });

  afterEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("renders dynamic workspace list under Projects in ConfigModal", () => {
    render(
      <ConfigModal
        onClose={vi.fn()}
        graphData={dummyGraphData}
        onUpdateGraphData={vi.fn()}
        bookmarks={[]}
        onPinArtifact={vi.fn()}
        onUpdateBookmark={vi.fn()}
      />,
    );

    expect(screen.getByText("Projects")).toBeInTheDocument();
    expect(screen.getByText("default")).toBeInTheDocument();
    expect(screen.getByText("swal")).toBeInTheDocument();
    expect(screen.getByText("personal")).toBeInTheDocument();
    expect(screen.getByText("work")).toBeInTheDocument();
  });

  it("switches active project, sets localStorage and dispatches xavier:workspace-changed custom event", async () => {
    const listener = vi.fn();
    window.addEventListener("xavier:workspace-changed", listener);

    render(
      <ConfigModal
        onClose={vi.fn()}
        graphData={dummyGraphData}
        onUpdateGraphData={vi.fn()}
        bookmarks={[]}
        onPinArtifact={vi.fn()}
        onUpdateBookmark={vi.fn()}
      />,
    );

    const swalBtn = screen.getByRole("button", { name: /swal/i });
    fireEvent.click(swalBtn);

    expect(localStorage.getItem("xavier_active_workspace")).toBe("swal");
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener.mock.calls[0][0].detail).toEqual({
      workspaceId: "swal",
    });

    window.removeEventListener("xavier:workspace-changed", listener);
  });

  it("adds new project via input prompt and switches to it", async () => {
    vi.spyOn(window, "prompt").mockReturnValue("my-new-project");

    render(
      <ConfigModal
        onClose={vi.fn()}
        graphData={dummyGraphData}
        onUpdateGraphData={vi.fn()}
        bookmarks={[]}
        onPinArtifact={vi.fn()}
        onUpdateBookmark={vi.fn()}
      />,
    );

    const addBtn = screen.getByRole("button", { name: /add project/i });
    fireEvent.click(addBtn);

    await waitFor(() => {
      expect(screen.getByText("my-new-project")).toBeInTheDocument();
    });

    expect(localStorage.getItem("xavier_active_workspace")).toBe(
      "my-new-project",
    );
    const workspacesInStore = JSON.parse(
      localStorage.getItem("xavier_workspaces") || "[]",
    );
    expect(workspacesInStore).toContain("my-new-project");
  });
});
