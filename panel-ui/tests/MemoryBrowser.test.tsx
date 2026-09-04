import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiClient } from "../src/api/client";
import MemoryBrowser from "../src/components/MemoryBrowser";

vi.mock("../src/api/client", () => {
  const ApiClientMock = vi.fn();
  ApiClientMock.prototype.searchMemories = vi.fn().mockResolvedValue([]);
  ApiClientMock.prototype.addMemory = vi.fn().mockResolvedValue({ id: "1" });
  ApiClientMock.prototype.exportMarkdown = vi.fn().mockResolvedValue({ export: "markdown content" });
  return { ApiClient: ApiClientMock };
});

describe("MemoryBrowser component", () => {
  const token = "test-token";

  beforeEach(() => {
    vi.clearAllMocks();
    // Mock URL.createObjectURL and URL.revokeObjectURL
    if (!window.URL.createObjectURL) {
      window.URL.createObjectURL = vi.fn().mockReturnValue("blob:http://localhost/test-blob");
    } else {
      vi.spyOn(window.URL, "createObjectURL").mockReturnValue("blob:http://localhost/test-blob");
    }

    if (!window.URL.revokeObjectURL) {
      window.URL.revokeObjectURL = vi.fn();
    } else {
      vi.spyOn(window.URL, "revokeObjectURL").mockImplementation(() => {});
    }
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders Memory Browser header and Export Markdown button", async () => {
    render(<MemoryBrowser token={token} />);
    expect(screen.getByText("Memory Browser")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Export Markdown/i })).toBeInTheDocument();
  });

  it("triggers exportMarkdown when Export Markdown button is clicked", async () => {
    const toastListener = vi.fn();
    window.addEventListener("xavier-error-toast", toastListener);

    render(<MemoryBrowser token={token} />);

    const exportBtn = screen.getByRole("button", { name: /Export Markdown/i });

    await act(async () => {
      fireEvent.click(exportBtn);
    });

    await waitFor(() => {
      expect(ApiClient.prototype.exportMarkdown).toHaveBeenCalledTimes(1);
      expect(toastListener).toHaveBeenCalledWith(
        expect.objectContaining({
          detail: { message: "Markdown export downloaded successfully!" },
        }),
      );
    });

    window.removeEventListener("xavier-error-toast", toastListener);
  });
});
