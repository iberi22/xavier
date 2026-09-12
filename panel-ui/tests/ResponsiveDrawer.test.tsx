import { fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import ResponsiveDrawer from "../src/components/ResponsiveDrawer";

describe("ResponsiveDrawer", () => {
  it("does not render when isOpen is false", () => {
    render(
      <ResponsiveDrawer isOpen={false} onClose={() => {}}>
        <div>Drawer Content</div>
      </ResponsiveDrawer>
    );
    expect(screen.queryByText("Drawer Content")).toBeNull();
  });

  it("renders children and title when isOpen is true", () => {
    render(
      <ResponsiveDrawer isOpen={true} onClose={() => {}} title="Settings Drawer">
        <div>Drawer Content</div>
      </ResponsiveDrawer>
    );
    expect(screen.getByText("Settings Drawer")).toBeInTheDocument();
    expect(screen.getByText("Drawer Content")).toBeInTheDocument();
  });

  it("calls onClose when backdrop or close button is clicked", () => {
    const handleClose = vi.fn();
    render(
      <ResponsiveDrawer isOpen={true} onClose={handleClose} title="Drawer Title">
        <div>Body</div>
      </ResponsiveDrawer>
    );

    const backdrop = screen.getByTestId("drawer-backdrop");
    fireEvent.click(backdrop);
    expect(handleClose).toHaveBeenCalledTimes(1);

    const closeButton = screen.getByRole("button", { name: /close drawer/i });
    fireEvent.click(closeButton);
    expect(handleClose).toHaveBeenCalledTimes(2);
  });

  it("calls onClose when Escape key is pressed", () => {
    const handleClose = vi.fn();
    render(
      <ResponsiveDrawer isOpen={true} onClose={handleClose} title="Drawer Title">
        <div>Body</div>
      </ResponsiveDrawer>
    );

    fireEvent.keyDown(window, { key: "Escape" });
    expect(handleClose).toHaveBeenCalledTimes(1);
  });

  it("applies safe-area inset class on container", () => {
    render(
      <ResponsiveDrawer isOpen={true} onClose={() => {}} title="Drawer Title">
        <div>Body</div>
      </ResponsiveDrawer>
    );

    const dialog = screen.getByRole("dialog");
    expect(dialog.className).toContain("pb-[env(safe-area-inset-bottom)]");
    expect(dialog.className).toContain("bg-[#0d0e10]");
  });
});
