import { render, screen, fireEvent, act } from "@testing-library/react";
import React from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import VoiceCallModal, {
  VoiceCallPeer,
} from "../src/components/Mesh/VoiceCallModal";

describe("VoiceCallModal component unit tests", () => {
  const mockPeer: VoiceCallPeer = {
    node_id: "node-123456789-abc",
    alias: "Peer Node One",
  };

  const mockOnEndCall = vi.fn();
  const mockOnAccept = vi.fn();
  const mockOnDecline = vi.fn();
  const mockOnToggleMute = vi.fn();
  const mockOnToggleSpeaker = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("does not render when isOpen is false", () => {
    const { container } = render(
      <VoiceCallModal
        isOpen={false}
        peer={mockPeer}
        direction="incoming"
        onEndCall={mockOnEndCall}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders incoming call state correctly with peer info", () => {
    render(
      <VoiceCallModal
        isOpen={true}
        peer={mockPeer}
        direction="incoming"
        onAccept={mockOnAccept}
        onDecline={mockOnDecline}
        onEndCall={mockOnEndCall}
      />,
    );

    expect(screen.getByText("Peer Node One")).toBeInTheDocument();
    expect(screen.getByText("node-123456789-abc")).toBeInTheDocument();
    expect(screen.getByText("Incoming P2P Audio Call...")).toBeInTheDocument();

    const acceptBtn = screen.getByRole("button", { name: "Accept voice call" });
    const declineBtn = screen.getByRole("button", {
      name: "Decline voice call",
    });

    expect(acceptBtn).toBeInTheDocument();
    expect(declineBtn).toBeInTheDocument();
  });

  it("handles accepting and declining an incoming call", () => {
    const { rerender } = render(
      <VoiceCallModal
        isOpen={true}
        peer={mockPeer}
        direction="incoming"
        onAccept={mockOnAccept}
        onDecline={mockOnDecline}
        onEndCall={mockOnEndCall}
      />,
    );

    const declineBtn = screen.getByRole("button", {
      name: "Decline voice call",
    });
    fireEvent.click(declineBtn);
    expect(mockOnDecline).toHaveBeenCalledTimes(1);

    const acceptBtn = screen.getByRole("button", { name: "Accept voice call" });
    fireEvent.click(acceptBtn);
    expect(mockOnAccept).toHaveBeenCalledTimes(1);
  });

  it("renders active outgoing call controls and handles mute/speaker toggles", () => {
    render(
      <VoiceCallModal
        isOpen={true}
        peer={mockPeer}
        direction="outgoing"
        connectionState="connected"
        onEndCall={mockOnEndCall}
        onToggleMute={mockOnToggleMute}
        onToggleSpeaker={mockOnToggleSpeaker}
      />,
    );

    expect(screen.getByText("Encrypted P2P Voice Active")).toBeInTheDocument();
    expect(screen.getByText("Encrypted P2P")).toBeInTheDocument();

    const muteBtn = screen.getByRole("button", { name: "Mute microphone" });
    const speakerBtn = screen.getByRole("button", { name: "Mute speaker" });
    const endCallBtn = screen.getByRole("button", { name: "End call" });

    fireEvent.click(muteBtn);
    expect(mockOnToggleMute).toHaveBeenCalledWith(true);
    expect(screen.getByRole("button", { name: "Unmute microphone" })).toBeInTheDocument();

    fireEvent.click(speakerBtn);
    expect(mockOnToggleSpeaker).toHaveBeenCalledWith(true);
    expect(screen.getByRole("button", { name: "Turn on speaker" })).toBeInTheDocument();

    fireEvent.click(endCallBtn);
    expect(mockOnEndCall).toHaveBeenCalledTimes(1);
  });

  it("displays WebRTC connection states accurately", () => {
    const { rerender } = render(
      <VoiceCallModal
        isOpen={true}
        peer={mockPeer}
        direction="outgoing"
        connectionState="connecting"
        onEndCall={mockOnEndCall}
      />,
    );

    expect(screen.getByText("Connecting WebRTC...")).toBeInTheDocument();
    expect(screen.getByText("Connecting...")).toBeInTheDocument();

    rerender(
      <VoiceCallModal
        isOpen={true}
        peer={mockPeer}
        direction="outgoing"
        connectionState="failed"
        onEndCall={mockOnEndCall}
      />,
    );

    expect(screen.getByText("Connection Failed")).toBeInTheDocument();
    expect(screen.getByText("Disconnected")).toBeInTheDocument();
  });

  it("handles microphone permission error gracefully when mediaDevices unavailable", async () => {
    const originalMediaDevices = navigator.mediaDevices;
    // @ts-ignore
    delete navigator.mediaDevices;

    await act(async () => {
      render(
        <VoiceCallModal
          isOpen={true}
          peer={mockPeer}
          direction="outgoing"
          onEndCall={mockOnEndCall}
        />,
      );
    });

    expect(screen.getByText("Microphone Issue")).toBeInTheDocument();
    expect(
      screen.getByText("Audio recording is not supported in this browser environment."),
    ).toBeInTheDocument();

    // Restore mediaDevices
    Object.defineProperty(navigator, "mediaDevices", {
      value: originalMediaDevices,
      writable: true,
      configurable: true,
    });
  });
});
