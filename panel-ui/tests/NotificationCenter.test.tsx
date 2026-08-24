import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
import React from "react";
import NotificationCenter, {
	Notification,
	ToastBanner,
} from "../src/components/NotificationCenter";

// Mock @tauri-apps/api
vi.mock("@tauri-apps/api/core", () => ({
	invoke: vi.fn().mockResolvedValue("mock-token"),
}));

vi.mock("@tauri-apps/api/event", () => ({
	listen: vi.fn().mockResolvedValue(() => {}),
}));

describe("NotificationCenter Component", () => {
	const mockNotifications: Notification[] = [
		{
			id: "notif-1",
			islandId: "system",
			title: "System Update Complete",
			body: "Xavier engine updated to latest version.",
			timestamp: new Date().toISOString(),
			read: false,
			severity: "info",
		},
		{
			id: "notif-2",
			islandId: "errors",
			title: "Connection Lost",
			body: "Failed to connect to P2P mesh bootstrap node.",
			timestamp: new Date().toISOString(),
			read: false,
			severity: "error",
		},
		{
			id: "notif-3",
			islandId: "agents",
			title: "Agent Task Finished",
			body: "Code generator completed feature task.",
			timestamp: new Date().toISOString(),
			read: true,
			severity: "success",
		},
	];

	const originalFetch = global.fetch;

	beforeEach(() => {
		global.fetch = vi.fn().mockImplementation((url: string) => {
			if (url.includes("/notifications")) {
				return Promise.resolve({
					ok: true,
					json: () => Promise.resolve(mockNotifications),
				} as Response);
			}
			return Promise.resolve({ ok: true, json: () => Promise.resolve({}) } as Response);
		});
	});

	afterEach(() => {
		global.fetch = originalFetch;
		vi.restoreAllMocks();
	});

	it("renders when isOpen is true", async () => {
		await act(async () => {
			render(
				<NotificationCenter
					isOpen={true}
					onClose={() => {}}
					initialNotifications={mockNotifications}
					enableWebSocket={false}
				/>,
			);
		});

		expect(screen.getByText("Notification Center")).toBeInTheDocument();
		expect(screen.getByText("System Update Complete")).toBeInTheDocument();
		expect(screen.getByText("Connection Lost")).toBeInTheDocument();
		expect(screen.getByText("Agent Task Finished")).toBeInTheDocument();
	});

	it("does not render drawer when isOpen is false", async () => {
		await act(async () => {
			render(
				<NotificationCenter
					isOpen={false}
					onClose={() => {}}
					initialNotifications={mockNotifications}
					enableWebSocket={false}
				/>,
			);
		});

		expect(screen.queryByText("Notification Center")).not.toBeInTheDocument();
	});

	it("filters notifications by island tab", async () => {
		await act(async () => {
			render(
				<NotificationCenter
					isOpen={true}
					onClose={() => {}}
					initialNotifications={mockNotifications}
					enableWebSocket={false}
				/>,
			);
		});

		// Click on Errors island tab
		const errorsTab = screen.getByRole("button", { name: /Errors/i });
		await act(async () => {
			fireEvent.click(errorsTab);
		});

		await waitFor(() => {
			expect(screen.getByText("Connection Lost")).toBeInTheDocument();
			expect(screen.queryByText("System Update Complete")).not.toBeInTheDocument();
			expect(screen.queryByText("Agent Task Finished")).not.toBeInTheDocument();
		});
	});

	it("marks notification as read", async () => {
		await act(async () => {
			render(
				<NotificationCenter
					isOpen={true}
					onClose={() => {}}
					initialNotifications={mockNotifications}
					enableWebSocket={false}
				/>,
			);
		});

		const markReadBtns = screen.getAllByRole("button", { name: /Mark as read/i });
		expect(markReadBtns.length).toBeGreaterThan(0);

		await act(async () => {
			fireEvent.click(markReadBtns[0]);
		});

		await waitFor(() => {
			expect(global.fetch).toHaveBeenCalledWith(
				expect.stringContaining("/notifications/notif-1/read"),
				expect.objectContaining({ method: "PATCH" }),
			);
		});
	});

	it("renders ToastBanner and handles dismiss", () => {
		const handleDismiss = vi.fn();
		const toast: Notification = {
			id: "toast-1",
			title: "New Alert",
			body: "Immediate alert message",
			timestamp: new Date().toISOString(),
			read: false,
			severity: "warning",
		};

		render(<ToastBanner toast={toast} onDismiss={handleDismiss} />);

		expect(screen.getByText("New Alert")).toBeInTheDocument();
		expect(screen.getByText("Immediate alert message")).toBeInTheDocument();

		const dismissBtn = screen.getByRole("button", {
			name: /Close toast notification/i,
		});
		fireEvent.click(dismissBtn);

		expect(handleDismiss).toHaveBeenCalledWith("toast-1");
	});
});
