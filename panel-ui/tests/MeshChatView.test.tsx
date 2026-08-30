import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import React from "react";
import MeshChatView, {
  ChatChannel,
  ChatMessage,
} from "../src/components/Mesh/MeshChatView";

describe("MeshChatView", () => {
  const customChannels: ChatChannel[] = [
    {
      id: "room-general",
      name: "general",
      type: "room",
      unreadCount: 2,
    },
    {
      id: "room-dev-council",
      name: "dev-council",
      type: "room",
    },
    {
      id: "peer-gamma",
      name: "Peer Gamma",
      alias: "Peer Gamma",
      nodeId: "node-g7719a",
      type: "direct",
      online: true,
    },
  ];

  const customMessages: ChatMessage[] = [
    {
      id: "msg-1",
      channelId: "room-general",
      senderAlias: "Node Alpha",
      senderNodeId: "node-a7f92b",
      content: "Encrypted P2P connection established across the mesh.",
      timestamp: "10:42 AM",
      encrypted: true,
      isSelf: false,
    },
    {
      id: "msg-2",
      channelId: "room-dev-council",
      senderAlias: "Peer Gamma",
      senderNodeId: "node-g7719a",
      content: "Dev council channel initial message.",
      timestamp: "11:00 AM",
      encrypted: true,
      isSelf: false,
    },
  ];

  it("renders channel switcher with Network Rooms and Direct Peer Chats", () => {
    render(<MeshChatView channels={customChannels} messages={customMessages} />);

    expect(screen.getByText("Network Rooms")).toBeInTheDocument();
    expect(screen.getByText("Direct Peer Chats")).toBeInTheDocument();

    expect(screen.getByText("general")).toBeInTheDocument();
    expect(screen.getByText("dev-council")).toBeInTheDocument();
    expect(screen.getByText("Peer Gamma")).toBeInTheDocument();
  });

  it("displays message stream with timestamp, sender alias, and encryption lock indicator", () => {
    render(<MeshChatView channels={customChannels} messages={customMessages} />);

    expect(screen.getByText("Encrypted P2P connection established across the mesh.")).toBeInTheDocument();
    expect(screen.getByText("Node Alpha")).toBeInTheDocument();
    expect(screen.getByText("(node-a7f92b)")).toBeInTheDocument();
    expect(screen.getByText("10:42 AM")).toBeInTheDocument();
    expect(screen.getByText("P2P Encrypted")).toBeInTheDocument();
  });

  it("allows switching between channels", () => {
    render(<MeshChatView channels={customChannels} messages={customMessages} />);

    // Click on dev-council room
    const devCouncilButton = screen.getByText("dev-council");
    fireEvent.click(devCouncilButton);

    expect(screen.getByText("Dev council channel initial message.")).toBeInTheDocument();
    expect(screen.queryByText("Encrypted P2P connection established across the mesh.")).not.toBeInTheDocument();
  });

  it("sends message on Enter keypress and calls onSendMessage callback", () => {
    const handleSendMessage = vi.fn();
    render(
      <MeshChatView
        channels={customChannels}
        messages={customMessages}
        onSendMessage={handleSendMessage}
      />
    );

    const input = screen.getByPlaceholderText(/Message #general.../i);
    fireEvent.change(input, { target: { value: "New encrypted message test" } });
    fireEvent.keyDown(input, { key: "Enter", code: "Enter" });

    expect(screen.getByText("New encrypted message test")).toBeInTheDocument();
    expect(handleSendMessage).toHaveBeenCalledWith(
      "room-general",
      "New encrypted message test",
      null
    );
    expect((input as HTMLInputElement).value).toBe("");
  });

  it("supports attachment capability indicator and file selection", () => {
    render(<MeshChatView channels={customChannels} messages={customMessages} />);

    const attachButton = screen.getByLabelText("Attach file to message");
    expect(attachButton).toBeInTheDocument();

    // Create a mock file
    const file = new File(["dummy content"], "test-artifact.json", {
      type: "application/json",
    });

    // File input is a sibling of the attach button in the parent container
    const fileInput = attachButton.parentElement?.querySelector('input[type="file"]') as HTMLInputElement;
    expect(fileInput).toBeInTheDocument();

    fireEvent.change(fileInput, { target: { files: [file] } });

    expect(screen.getByText("test-artifact.json")).toBeInTheDocument();
  });
});
