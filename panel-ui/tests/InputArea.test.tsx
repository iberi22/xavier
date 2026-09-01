import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import React from "react";
import InputArea from "../src/components/InputArea";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

describe("InputArea component unit tests", () => {
  const onSendMessage = vi.fn();
  const onOpenConfig = vi.fn();
  const onSystemMessage = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    // Enable Tauri mode for existing tests in this file
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      writable: true,
      configurable: true,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("calls onOpenConfig when config button is clicked", () => {
    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const configBtn = screen.getByRole("button", { name: "Open Control Node" });
    fireEvent.click(configBtn);

    expect(onOpenConfig).toHaveBeenCalledTimes(1);
  });

  it("handles typing and sending text on send button click", () => {
    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const input = screen.getByRole("textbox", { name: "Command input" });
    const sendBtn = screen.getByRole("button", { name: "Send command" });

    // Send button disabled when empty
    expect(sendBtn).toBeDisabled();

    fireEvent.change(input, { target: { value: "Hello world" } });
    expect(sendBtn).not.toBeDisabled();

    fireEvent.click(sendBtn);

    expect(onSendMessage).toHaveBeenCalledWith("Hello world");
    expect((input as HTMLInputElement).value).toBe("");
  });

  it("handles sending text when Enter key is pressed", () => {
    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const input = screen.getByRole("textbox", { name: "Command input" });

    fireEvent.change(input, { target: { value: "Enter test" } });
    fireEvent.keyDown(input, { key: "Enter", code: "Enter" });

    expect(onSendMessage).toHaveBeenCalledWith("Enter test");
  });

  it("does not send message if text is only whitespace", () => {
    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const input = screen.getByRole("textbox", { name: "Command input" });
    const sendBtn = screen.getByRole("button", { name: "Send command" });

    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.click(sendBtn);

    expect(onSendMessage).not.toHaveBeenCalled();
  });

  it("handles project folder selection successfully", async () => {
    (open as unknown as ReturnType<typeof vi.fn>).mockResolvedValue("/path/to/project");
    (invoke as unknown as ReturnType<typeof vi.fn>).mockResolvedValue("Project scanned successfully");

    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const folderBtn = screen.getByRole("button", { name: "Add project codebase" });
    await act(async () => {
      fireEvent.click(folderBtn);
    });

    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false });
    expect(onSystemMessage).toHaveBeenCalledWith("Iniciando escaneo del proyecto en: /path/to/project...");
    expect(invoke).toHaveBeenCalledWith("scan_project_folder", { path: "/path/to/project" });
    expect(onSystemMessage).toHaveBeenCalledWith("✅ Project scanned successfully");
  });

  it("handles cancelled project folder selection", async () => {
    (open as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(null);

    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const folderBtn = screen.getByRole("button", { name: "Add project codebase" });
    await act(async () => {
      fireEvent.click(folderBtn);
    });

    expect(open).toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("handles project folder scan error", async () => {
    (open as unknown as ReturnType<typeof vi.fn>).mockResolvedValue("/path/to/project");
    (invoke as unknown as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("Scan failed"));

    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const folderBtn = screen.getByRole("button", { name: "Add project codebase" });
    await act(async () => {
      fireEvent.click(folderBtn);
    });

    expect(onSystemMessage).toHaveBeenCalledWith("❌ Error al escanear: Error: Scan failed");
  });

  it("handles mic recording toggle and transcription simulation", () => {
    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const micBtn = screen.getByRole("button", { name: "Record audio" });

    // Click to start recording
    fireEvent.click(micBtn);
    expect(screen.getByRole("button", { name: "Stop recording" })).toBeInTheDocument();

    // Click to stop recording and start transcription
    fireEvent.click(screen.getByRole("button", { name: "Stop recording" }));
    expect(screen.getByText("Transcribing")).toBeInTheDocument();

    // Advance fake timers by 2000ms
    act(() => {
      vi.advanceTimersByTime(2000);
    });

    expect(screen.queryByText("Transcribing")).not.toBeInTheDocument();
    const input = screen.getByRole("textbox", { name: "Command input" });
    expect((input as HTMLInputElement).value).toContain("Audio transcript processed.");
  });
});
