import { fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import { RecoveryModal } from "../src/components/RecoveryModal";

describe("RecoveryModal Component", () => {
  const sampleMnemonic =
    "abandon amount abandon amount abandon amount abandon amount abandon amount abandon announce";
  const defaultProps = {
    isOpen: true,
    onClose: vi.fn(),
    mnemonic: sampleMnemonic,
    onRestore: vi.fn(),
  };

  it("renders correctly when isOpen is true", () => {
    render(<RecoveryModal {...defaultProps} />);
    expect(screen.getByText("Disaster Recovery Wizard")).toBeInTheDocument();
    expect(
      screen.getByText("Zero-Knowledge Sovereign Custody Notice")
    ).toBeInTheDocument();
    expect(
      screen.getByText("1. Save Your Mnemonic Words")
    ).toBeInTheDocument();
  });

  it("does not render when isOpen is false", () => {
    render(<RecoveryModal {...defaultProps} isOpen={false} />);
    expect(
      screen.queryByText("Disaster Recovery Wizard")
    ).not.toBeInTheDocument();
  });

  it("switches tabs between Backup Node and Restore Node", () => {
    render(<RecoveryModal {...defaultProps} />);

    // Initially in Backup tab
    expect(screen.getByText("2. Backup Verification Quiz")).toBeInTheDocument();

    // Click Restore tab
    const restoreTabBtn = screen.getByRole("button", { name: /restore node/i });
    fireEvent.click(restoreTabBtn);

    expect(
      screen.getByText("Sovereign Node Reconstruction")
    ).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText(/abandon amount abandon amount/i)
    ).toBeInTheDocument();

    // Click Backup tab again
    const backupTabBtn = screen.getByRole("button", { name: /backup node/i });
    fireEvent.click(backupTabBtn);

    expect(screen.getByText("2. Backup Verification Quiz")).toBeInTheDocument();
  });

  it("validates quiz inputs correctly and enables the confirmation button only on correct inputs", () => {
    render(<RecoveryModal {...defaultProps} />);

    const confirmBtn = screen.getByRole("button", {
      name: /i have securely backed up my key/i,
    });
    expect(confirmBtn).toBeDisabled();

    // Retrieve requested word indices from text (e.g. "To confirm ... type word #X and word #Y.")
    const quizText = screen.getByText(/To confirm you have stored your phrase/i).textContent || "";
    const matches = quizText.match(/#(\d+)/g);
    expect(matches).not.toBeNull();
    const idx1 = parseInt(matches![0].replace("#", ""), 10) - 1;
    const idx2 = parseInt(matches![1].replace("#", ""), 10) - 1;

    const words = sampleMnemonic.split(" ");
    const word1 = words[idx1];
    const word2 = words[idx2];

    const input1 = screen.getByPlaceholderText(`Enter word #${idx1 + 1}`);
    const input2 = screen.getByPlaceholderText(`Enter word #${idx2 + 1}`);

    // Enter wrong answer
    fireEvent.change(input1, { target: { value: "wrongword" } });
    fireEvent.change(input2, { target: { value: "wrongword" } });
    expect(
      screen.getByText("Words do not match your backup mnemonic phrase.")
    ).toBeInTheDocument();
    expect(confirmBtn).toBeDisabled();

    // Enter correct answers
    fireEvent.change(input1, { target: { value: word1 } });
    fireEvent.change(input2, { target: { value: word2 } });

    expect(
      screen.getByText("Word verification passed!")
    ).toBeInTheDocument();
    expect(confirmBtn).not.toBeDisabled();

    // Click confirmation button
    fireEvent.click(confirmBtn);
    expect(defaultProps.onClose).toHaveBeenCalled();
  });

  it("validates restore seed phrase length and BIP-39 format before triggering onRestore", () => {
    const onRestoreMock = vi.fn();
    render(<RecoveryModal {...defaultProps} onRestore={onRestoreMock} />);

    // Switch to restore tab
    fireEvent.click(screen.getByRole("button", { name: /restore node/i }));

    const textarea = screen.getByPlaceholderText(/abandon amount abandon amount/i);
    const submitBtn = screen.getByRole("button", {
      name: /restore sovereign node identity/i,
    });

    // Test short/invalid word count phrase
    fireEvent.change(textarea, { target: { value: "abandon amount abandon" } });
    fireEvent.click(submitBtn);

    expect(
      screen.getByText(/Invalid seed phrase length: 3 words found/i)
    ).toBeInTheDocument();
    expect(onRestoreMock).not.toHaveBeenCalled();

    // Test phrase with invalid characters/numbers
    fireEvent.change(textarea, {
      target: {
        value:
          "abandon amount abandon amount abandon amount abandon amount abandon amount abandon 12345",
      },
    });
    fireEvent.click(submitBtn);

    expect(
      screen.getByText(/Seed phrase contains invalid characters or words/i)
    ).toBeInTheDocument();
    expect(onRestoreMock).not.toHaveBeenCalled();

    // Test valid 12-word phrase submission
    fireEvent.change(textarea, { target: { value: sampleMnemonic } });
    fireEvent.click(submitBtn);

    expect(onRestoreMock).toHaveBeenCalledWith(sampleMnemonic);
  });

  it("calls onClose when close button or Escape key is pressed", () => {
    const onCloseMock = vi.fn();
    render(<RecoveryModal {...defaultProps} onClose={onCloseMock} />);

    const closeBtn = screen.getByRole("button", { name: "Cerrar" });
    fireEvent.click(closeBtn);
    expect(onCloseMock).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onCloseMock).toHaveBeenCalledTimes(2);
  });
});
