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

describe("InputArea browser File API fallback unit tests", () => {
  const onSendMessage = vi.fn();
  const onOpenConfig = vi.fn();
  const onSystemMessage = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  afterEach(() => {
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("clicks hidden file input when FolderPlus button is clicked in browser mode", () => {
    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const hiddenInput = document.querySelector('input[type="file"]') as HTMLInputElement;
    expect(hiddenInput).toBeInTheDocument();

    const clickSpy = vi.spyOn(hiddenInput, "click");
    const folderBtn = screen.getByRole("button", { name: "Add project codebase" });

    fireEvent.click(folderBtn);

    expect(clickSpy).toHaveBeenCalledTimes(1);
    expect(open).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("constructs correct folder name and calls onSystemMessage when files are selected via onChange", () => {
    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const hiddenInput = document.querySelector('input[type="file"]') as HTMLInputElement;

    const mockFile1 = new File(["content1"], "index.ts");
    Object.defineProperty(mockFile1, "webkitRelativePath", {
      value: "my-cool-project/src/index.ts",
    });

    const mockFile2 = new File(["content2"], "package.json");
    Object.defineProperty(mockFile2, "webkitRelativePath", {
      value: "my-cool-project/package.json",
    });

    const mockFile3 = new File(["content3"], "README.md");
    Object.defineProperty(mockFile3, "webkitRelativePath", {
      value: "my-cool-project/README.md",
    });

    fireEvent.change(hiddenInput, {
      target: {
        files: [mockFile1, mockFile2, mockFile3],
      },
    });

    expect(onSystemMessage).toHaveBeenCalledWith(
      "Carpeta seleccionada: my-cool-project (3 archivos)"
    );
  });

  it("uses dynamic open and invoke mocks when in Tauri mode", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      writable: true,
      configurable: true,
    });

    (open as unknown as ReturnType<typeof vi.fn>).mockResolvedValue("/tauri/project/path");
    (invoke as unknown as ReturnType<typeof vi.fn>).mockResolvedValue("Escaneo completado exitosamente");

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
    expect(onSystemMessage).toHaveBeenCalledWith(
      "Iniciando escaneo del proyecto en: /tauri/project/path..."
    );
    expect(invoke).toHaveBeenCalledWith("scan_project_folder", {
      path: "/tauri/project/path",
    });
    expect(onSystemMessage).toHaveBeenCalledWith("✅ Escaneo completado exitosamente");
  });

  it("does not crash when onSystemMessage is undefined during file selection and folder click", () => {
    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
      />
    );

    const folderBtn = screen.getByRole("button", { name: "Add project codebase" });
    const hiddenInput = document.querySelector('input[type="file"]') as HTMLInputElement;

    expect(() => {
      fireEvent.click(folderBtn);
    }).not.toThrow();

    const mockFile = new File(["content"], "test.txt");
    Object.defineProperty(mockFile, "webkitRelativePath", {
      value: "test-dir/test.txt",
    });

    expect(() => {
      fireEvent.change(hiddenInput, {
        target: {
          files: [mockFile],
        },
      });
    }).not.toThrow();
  });

  it("has aria-hidden='true' on the hidden file input element", () => {
    render(
      <InputArea
        onSendMessage={onSendMessage}
        onOpenConfig={onOpenConfig}
        onSystemMessage={onSystemMessage}
      />
    );

    const hiddenInput = document.querySelector('input[type="file"]') as HTMLInputElement;
    expect(hiddenInput).toHaveAttribute("aria-hidden", "true");
    expect(hiddenInput).toHaveStyle({ display: "none" });
    expect(hiddenInput).toHaveAttribute("webkitdirectory", "");
  });
});
