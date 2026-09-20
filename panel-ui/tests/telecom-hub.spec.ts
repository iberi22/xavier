import {
	act,
	fireEvent,
	render,
	renderHook,
	screen,
} from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MeshChatView, {
	type ChatChannel,
	type ChatMessage,
} from "../src/components/Mesh/MeshChatView";
import MeshHubView from "../src/components/Mesh/MeshHubView";
import { useXavierWebSocket } from "../src/hooks/useXavierWebSocket";

// Mock Tauri invoke & listen API
vi.mock("@tauri-apps/api/core", () => ({
	invoke: vi.fn().mockImplementation(async (cmd: string) => {
		if (cmd === "get_current_config_state") {
			return { has_openai: true, has_gemini: false, has_telegram: false };
		}
		if (cmd === "get_realtime_metrics") {
			return { cpu_percent: 15, ram_used_gb: 4, ram_total_gb: 16 };
		}
		return null;
	}),
}));

vi.mock("@tauri-apps/api/event", () => ({
	listen: vi.fn().mockReturnValue(Promise.resolve(() => {})),
}));

// Mock WebSocket implementation for frame handling tests
class MockWebSocket {
	static instances: MockWebSocket[] = [];
	static OPEN = 1;
	static CLOSED = 3;

	url: string;
	readyState: number = MockWebSocket.OPEN;
	onopen: (() => void) | null = null;
	onmessage: ((event: { data: string }) => void) | null = null;
	onerror: ((error: unknown) => void) | null = null;
	onclose: (() => void) | null = null;
	sentData: string[] = [];

	constructor(url: string) {
		this.url = url;
		MockWebSocket.instances.push(this);
		setTimeout(() => {
			if (this.onopen) this.onopen();
		}, 0);
	}

	send(data: string) {
		this.sentData.push(data);
	}

	close() {
		this.readyState = MockWebSocket.CLOSED;
		if (this.onclose) this.onclose();
	}

	// Helper to simulate receiving WebSocket frames from server
	receiveFrame(data: object) {
		if (this.onmessage) {
			this.onmessage({ data: JSON.stringify(data) });
		}
	}
}

describe("TelecomHub & DirectChat Component Interactions", () => {
	const originalWebSocket = global.WebSocket;

	beforeEach(() => {
		MockWebSocket.instances = [];
		(global as unknown as { WebSocket: typeof MockWebSocket }).WebSocket =
			MockWebSocket;
	});

	afterEach(() => {
		global.WebSocket = originalWebSocket;
		vi.clearAllMocks();
	});

	describe("TelecomHub & MeshHubView Navigation & Tab Transitions", () => {
		it("renders initial view with network indicator and tab navigation", () => {
			render(React.createElement(MeshHubView, { token: "mock-telecom-token" }));

			expect(screen.getByText("Xavier Mesh Hub")).toBeInTheDocument();
			expect(
				screen.getByText("Active Network: P2P-Mesh-Mainnet"),
			).toBeInTheDocument();

			expect(
				screen.getByRole("button", { name: /Networks/i }),
			).toBeInTheDocument();
			expect(
				screen.getByRole("button", { name: /Topology/i }),
			).toBeInTheDocument();
			expect(
				screen.getByRole("button", { name: /DAO Governance/i }),
			).toBeInTheDocument();
			expect(
				screen.getByRole("button", { name: /P2P Chat/i }),
			).toBeInTheDocument();
			expect(
				screen.getByRole("button", { name: /Family Health/i }),
			).toBeInTheDocument();
		});

		it("handles tab transitions correctly across all hub tabs", () => {
			render(
				React.createElement(MeshHubView, {
					token: "mock-telecom-token",
					initialTab: "networks",
				}),
			);

			// Switch to Topology tab
			fireEvent.click(screen.getByRole("button", { name: /Topology/i }));
			expect(screen.getByText("Mesh Network Topology")).toBeInTheDocument();
			expect(
				screen.getByText("Live Peer Connections Graph"),
			).toBeInTheDocument();

			// Switch to Governance tab
			fireEvent.click(screen.getByRole("button", { name: /DAO Governance/i }));
			expect(
				screen.getByText("DAO Governance & Tokenomics"),
			).toBeInTheDocument();
			expect(
				screen.getByText(
					"Upgrade Mesh Sync Protocol to v2.4 (Quorum Consensus)",
				),
			).toBeInTheDocument();

			// Switch to P2P Chat tab
			fireEvent.click(screen.getByRole("button", { name: /P2P Chat/i }));
			expect(screen.getByText("Encrypted Mesh P2P Chat")).toBeInTheDocument();

			// Switch to Family Health tab
			fireEvent.click(screen.getByRole("button", { name: /Family Health/i }));
			expect(
				screen.getByText("Family Node Health & Auto-Repair Module"),
			).toBeInTheDocument();
			expect(screen.getByText("Primary Gateway")).toBeInTheDocument();
		});
	});

	describe("DirectChat & Message Rendering", () => {
		const mockChannels: ChatChannel[] = [
			{
				id: "room-telecom-1",
				name: "telecom-ops",
				type: "room",
				unreadCount: 1,
			},
			{
				id: "peer-node-alpha",
				name: "Alpha Relay",
				alias: "Alpha Relay",
				nodeId: "node-a1b2c3",
				type: "direct",
				online: true,
			},
		];

		const mockMessages: ChatMessage[] = [
			{
				id: "msg-telecom-1",
				channelId: "room-telecom-1",
				senderAlias: "Telecom Dispatcher",
				senderNodeId: "node-d001",
				content: "Direct channel encrypted link initialized.",
				timestamp: "12:00 PM",
				encrypted: true,
				isSelf: false,
			},
		];

		it("renders direct channels, encryption lock status, and message streams", () => {
			render(
				React.createElement(MeshChatView, {
					channels: mockChannels,
					messages: mockMessages,
				}),
			);

			expect(screen.getByText("telecom-ops")).toBeInTheDocument();
			expect(screen.getByText("Alpha Relay")).toBeInTheDocument();
			expect(
				screen.getByText("Direct channel encrypted link initialized."),
			).toBeInTheDocument();
			expect(screen.getByText("P2P Encrypted")).toBeInTheDocument();
		});

		it("allows sending new messages via form submit and invokes onSendMessage", () => {
			const handleSendMessage = vi.fn();
			render(
				React.createElement(MeshChatView, {
					channels: mockChannels,
					messages: mockMessages,
					onSendMessage: handleSendMessage,
				}),
			);

			const input = screen.getByPlaceholderText(/Message #telecom-ops.../i);
			fireEvent.change(input, {
				target: { value: "Telemetry frame broadcast test" },
			});
			fireEvent.keyDown(input, { key: "Enter", code: "Enter" });

			expect(
				screen.getByText("Telemetry frame broadcast test"),
			).toBeInTheDocument();
			expect(handleSendMessage).toHaveBeenCalledWith(
				"room-telecom-1",
				"Telemetry frame broadcast test",
				null,
			);
		});
	});

	describe("Mock WebSocket Frame Handling", () => {
		it("establishes connection and handles XUI event dispatch over WebSocket", async () => {
			const threadId = "telecom-thread-999";
			const { result } = renderHook(() => useXavierWebSocket(threadId));

			expect(MockWebSocket.instances.length).toBe(1);
			const wsInstance = MockWebSocket.instances[0];
			expect(wsInstance.url).toBe(`ws://localhost:8006/ws/panel/${threadId}`);

			// Dispatch XUI Event over WebSocket
			act(() => {
				result.current.sendXUIEvent({
					type: "action",
					componentId: "telecom-btn-1",
					componentType: "button",
					payload: { action: "sync_mesh" },
					timestamp: 1700000000,
				});
			});

			expect(wsInstance.sentData.length).toBe(1);
			const sentPayload = JSON.parse(wsInstance.sentData[0]);
			expect(sentPayload).toEqual({
				type: "xui_event",
				event_type: "action",
				componentId: "telecom-btn-1",
				componentType: "button",
				payload: { action: "sync_mesh" },
				timestamp: 1700000000,
				thread_id: threadId,
			});
		});

		it("handles incoming WebSocket frame message events cleanly", () => {
			const consoleSpy = vi.spyOn(console, "log").mockImplementation(() => {});
			const threadId = "telecom-thread-888";

			renderHook(() => useXavierWebSocket(threadId));
			const wsInstance = MockWebSocket.instances[0];

			// Simulate incoming server frame
			act(() => {
				wsInstance.receiveFrame({
					type: "telecom_status",
					connectedPeers: 5,
					meshState: "synced",
				});
			});

			expect(consoleSpy).toHaveBeenCalledWith(
				"[XavierWS] Received:",
				expect.objectContaining({ type: "telecom_status", connectedPeers: 5 }),
			);

			consoleSpy.mockRestore();
		});
	});
});
