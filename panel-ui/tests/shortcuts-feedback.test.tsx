import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ConfigModal from "../src/components/ConfigModal";

const dummyGraphData = {
  nodes: [],
  links: [],
};

describe("ConfigModal Shortcuts & Provide Feedback Dialogs", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("opens ShortcutsModal when clicking Shortcuts footer button and closes it on Escape", async () => {
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

    const shortcutsBtn = screen.getByRole("button", { name: "Shortcuts" });
    fireEvent.click(shortcutsBtn);

    expect(screen.getByText("Keyboard Shortcuts")).toBeInTheDocument();
    expect(screen.getByText("Quick navigation & action key bindings")).toBeInTheDocument();
    expect(screen.getByText("Focus prompt and command palette")).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByText("Keyboard Shortcuts")).not.toBeInTheDocument();
    });
  });

  it("opens FeedbackModal when clicking Provide Feedback and persists feedback to localStorage", async () => {
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

    const feedbackBtn = screen.getByRole("button", { name: "Provide Feedback" });
    fireEvent.click(feedbackBtn);

    expect(screen.getByText("Help us improve the Antigravity experience")).toBeInTheDocument();
    const textarea = screen.getByLabelText("Feedback comments");
    fireEvent.change(textarea, { target: { value: "Great Antigravity UI theme!" } });

    const submitBtn = screen.getByRole("button", { name: /submit/i });
    fireEvent.click(submitBtn);

    const storedFeedbacks = JSON.parse(localStorage.getItem("xavier_feedbacks") || "[]");
    expect(storedFeedbacks.length).toBe(1);
    expect(storedFeedbacks[0].text).toBe("Great Antigravity UI theme!");
  });
});
