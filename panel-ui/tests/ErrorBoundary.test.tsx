import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import React, { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "../src/components/ErrorBoundary";

// Component that throws on demand
function ProblematicComponent({ shouldThrow }: { shouldThrow: boolean }) {
  if (shouldThrow) {
    throw new Error("Test rendering exception in child component");
  }
  return <div data-testid="child-content">Normal Component Content</div>;
}

describe("ErrorBoundary Component", () => {
  // Silence console.error logs for caught React error boundary errors during testing
  const originalConsoleError = console.error;
  beforeEach(() => {
    console.error = vi.fn();
    localStorage.clear();
    vi.restoreAllMocks();
  });

  afterEach(() => {
    console.error = originalConsoleError;
    localStorage.clear();
  });

  it("renders children when no error occurs", () => {
    render(
      <ErrorBoundary>
        <ProblematicComponent shouldThrow={false} />
      </ErrorBoundary>,
    );

    expect(screen.getByTestId("child-content")).toBeInTheDocument();
    expect(screen.getByText("Normal Component Content")).toBeInTheDocument();
  });

  it("catches rendering error and displays Antigravity Studio Dark fallback UI", () => {
    render(
      <ErrorBoundary>
        <ProblematicComponent shouldThrow={true} />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Application Rendering Exception")).toBeInTheDocument();
    expect(
      screen.getByText(/Test rendering exception in child component/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Reload Interface/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Copy Error Diagnostic/i }),
    ).toBeInTheDocument();
  });

  it("invokes onError callback when error is caught", () => {
    const onErrorMock = vi.fn();

    render(
      <ErrorBoundary onError={onErrorMock}>
        <ProblematicComponent shouldThrow={true} />
      </ErrorBoundary>,
    );

    expect(onErrorMock).toHaveBeenCalledTimes(1);
    expect(onErrorMock.mock.calls[0][0].message).toBe(
      "Test rendering exception in child component",
    );
  });

  it("renders custom fallback node or fallback function", () => {
    render(
      <ErrorBoundary
        fallback={(error, reset) => (
          <div>
            <h1>Custom Fallback: {error.message}</h1>
            <button type="button" onClick={reset}>
              Try Again
            </button>
          </div>
        )}
      >
        <ProblematicComponent shouldThrow={true} />
      </ErrorBoundary>,
    );

    expect(
      screen.getByText("Custom Fallback: Test rendering exception in child component"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try Again" })).toBeInTheDocument();
  });

  it("triggers copy error diagnostic clipboard action", async () => {
    const writeTextMock = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: writeTextMock },
      writable: true,
      configurable: true,
    });

    render(
      <ErrorBoundary>
        <ProblematicComponent shouldThrow={true} />
      </ErrorBoundary>,
    );

    const copyBtn = screen.getByRole("button", { name: /Copy Error Diagnostic/i });
    fireEvent.click(copyBtn);

    await waitFor(() => {
      expect(writeTextMock).toHaveBeenCalledTimes(1);
      const copiedText = writeTextMock.mock.calls[0][0];
      expect(copiedText).toContain("Test rendering exception in child component");
      expect(screen.getByText("Copied!")).toBeInTheDocument();
    });
  });

  it("triggers reload handler when Reload Interface button is clicked", () => {
    const onReloadMock = vi.fn();

    render(
      <ErrorBoundary onReload={onReloadMock}>
        <ProblematicComponent shouldThrow={true} />
      </ErrorBoundary>,
    );

    const reloadBtn = screen.getByRole("button", { name: /Reload Interface/i });
    fireEvent.click(reloadBtn);

    expect(onReloadMock).toHaveBeenCalledTimes(1);
  });
});
