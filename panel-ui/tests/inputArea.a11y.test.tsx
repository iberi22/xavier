import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import React from "react";
import InputArea from "../src/components/InputArea";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

describe("InputArea accessibility", () => {
  it("exposes aria-labels on icon-only controls and command input", () => {
    render(
      <InputArea onSendMessage={vi.fn()} onOpenConfig={vi.fn()} />,
    );

    expect(
      screen.getByRole("button", { name: "Open Control Node" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Add project codebase" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Record audio" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Send command" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("textbox", { name: "Command input" }),
    ).toBeInTheDocument();
  });

  it("marks mic as pressed while recording", () => {
    render(
      <InputArea onSendMessage={vi.fn()} onOpenConfig={vi.fn()} />,
    );

    const mic = screen.getByRole("button", { name: "Record audio" });
    expect(mic).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(mic);

    const stopMic = screen.getByRole("button", { name: "Stop recording" });
    expect(stopMic).toHaveAttribute("aria-pressed", "true");
  });
});
