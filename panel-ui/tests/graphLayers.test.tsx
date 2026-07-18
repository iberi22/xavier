import { vi } from "vitest";
import React from "react";

// 1. Declare ALL mocks at the absolute top of the file
vi.mock("../src/auth/AuthProvider", () => ({
  useAuthStore: vi.fn((selector) => selector({
    token: "mock-token",
    isAuthenticated: true,
  })),
}));

vi.mock("motion/react", () => {
  const dummy = React.forwardRef(({ children, ...props }: any, ref: any) => {
    const { exit, animate, initial, transition, layoutId, ...rest } = props;
    return <div ref={ref} {...rest}>{children}</div>;
  });
  return {
    motion: {
      div: dummy,
      button: dummy,
      span: dummy,
      svg: dummy,
      path: dummy,
    },
    AnimatePresence: ({ children }: any) => <>{children}</>,
  };
});

vi.mock("react-force-graph-2d", () => {
  return {
    default: React.forwardRef(({ graphData, onNodeClick }: any, ref: any) => (
      <div data-testid="force-graph-mock">
        {graphData?.nodes?.map((node: any) => (
          <button
            key={node.id}
            data-testid={`node-${node.id}`}
            onClick={(e) => {
              e.stopPropagation();
              if (onNodeClick) onNodeClick(node);
            }}
          >
            {node.label || node.id}
          </button>
        ))}
      </div>
    )),
  };
});

// Mock resize observer
class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
global.ResizeObserver = ResizeObserverMock as any;

// 2. Now import testing library and components
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import ConfigModal from "../src/components/ConfigModal";
import type { GraphData } from "../src/types";

describe("ConfigModal Multi-Layer Graph Explorer", () => {
  const originalFetch = global.fetch;

  const mockGraphData: GraphData = {
    nodes: [
      { id: "org1", label: "Swal Corp", type: "organization", description: "Root organization" }
    ],
    links: []
  };

  const mockOnUpdateGraphData = vi.fn();
  const mockOnClose = vi.fn();
  const mockOnPinArtifact = vi.fn();
  const mockOnUpdateBookmark = vi.fn();

  beforeEach(() => {
    mockOnUpdateGraphData.mockClear();
    mockOnClose.mockClear();
  });

  afterEach(() => {
    global.fetch = originalFetch;
    vi.restoreAllMocks();
  });

  it("renders tab buttons and transitions to Roadmap layer successfully", async () => {
    render(
      <ConfigModal
        onClose={mockOnClose}
        graphData={mockGraphData}
        onUpdateGraphData={mockOnUpdateGraphData}
        bookmarks={[]}
        onPinArtifact={mockOnPinArtifact}
        onUpdateBookmark={mockOnUpdateBookmark}
        token="mock-token"
      />
    );

    // Switch to Roadmap top-level tab (the first button with text "Roadmap")
    const roadmapTabButtons = screen.getAllByRole("button", { name: /^Roadmap$/i });
    expect(roadmapTabButtons.length).toBeGreaterThan(0);
    fireEvent.click(roadmapTabButtons[0]);

    // Sub-tab sub-layers: Roadmap, Memory KG, Code should be visible
    expect(await screen.findByRole("tab", { name: /Roadmap/i })).toBeDefined();
    expect(await screen.findByRole("tab", { name: /Memory KG/i })).toBeDefined();
    expect(await screen.findByRole("tab", { name: /Code/i })).toBeDefined();
  });

  it("fetches and renders Memory KG sub-layer with correct empty/truncated states", async () => {
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (url.endsWith("/memory/graph/view")) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({
            nodes: [
              { id: "mem1", label: "Semantic Concept", type: "concept", description: "A test concept" }
            ],
            links: [],
            truncated: true,
          }),
        } as Response);
      }
      return Promise.resolve({ ok: false } as Response);
    });

    render(
      <ConfigModal
        onClose={mockOnClose}
        graphData={mockGraphData}
        onUpdateGraphData={mockOnUpdateGraphData}
        bookmarks={[]}
        onPinArtifact={mockOnPinArtifact}
        onUpdateBookmark={mockOnUpdateBookmark}
        token="mock-token"
      />
    );

    // Switch to Roadmap main tab
    const roadmapTabButtons = screen.getAllByRole("button", { name: /^Roadmap$/i });
    fireEvent.click(roadmapTabButtons[0]);

    // Switch to Memory KG sub-layer tab using findByRole (async-safe)
    const memoryKgTab = await screen.findByRole("tab", { name: /Memory KG/i });
    fireEvent.click(memoryKgTab);

    // Verify loading/fetch was initiated
    expect(global.fetch).toHaveBeenCalled();

    // Verify truncated badge is displayed
    await waitFor(() => {
      expect(screen.getByText(/TRUNCATED/i)).toBeDefined();
    });
  });

  it("fetches Code stats and handles empty code CTA scan trigger correctly", async () => {
    let scanCalled = false;
    global.fetch = vi.fn().mockImplementation((url: string, options?: any) => {
      if (url.endsWith("/code/stats")) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({
            total_symbols: 0,
            total_files: 0,
          }),
        } as Response);
      }
      if (url.endsWith("/code/scan") && options?.method === "POST") {
        scanCalled = true;
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ status: "ok" }),
        } as Response);
      }
      return Promise.resolve({ ok: false } as Response);
    });

    render(
      <ConfigModal
        onClose={mockOnClose}
        graphData={mockGraphData}
        onUpdateGraphData={mockOnUpdateGraphData}
        bookmarks={[]}
        onPinArtifact={mockOnPinArtifact}
        onUpdateBookmark={mockOnUpdateBookmark}
        token="mock-token"
      />
    );

    // Switch to Roadmap main tab
    const roadmapTabButtons = screen.getAllByRole("button", { name: /^Roadmap$/i });
    fireEvent.click(roadmapTabButtons[0]);

    // Switch to Code sub-layer using findByRole (async-safe)
    const codeTab = await screen.findByRole("tab", { name: /Code/i });
    fireEvent.click(codeTab);

    // Should render "Scan Codebase" CTA button
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Scan Now/i })).toBeDefined();
    });

    // Fire Scan CTA trigger
    fireEvent.click(screen.getByRole("button", { name: /Scan Now/i }));

    await waitFor(() => {
      expect(scanCalled).toBe(true);
    });
  });
});
