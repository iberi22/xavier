import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import NotificationCenter, { Notification } from "../src/components/NotificationCenter";
import NotificationsDropdown from "../src/components/NotificationsDropdown";

describe("Notifications Browser Compatibility & Functionality", () => {
	const mockNotifications: Notification[] = [
		{
			id: "notif-env-1",
			islandId: "system",
			title: "Env Token Notification",
			body: "Fetched using environment token.",
			timestamp: new Date().toISOString(),
			read: false,
			severity: "info",
		},
		{
			id: "notif-env-2",
			islandId: "errors",
			title: "Error Notification",
			body: "System error occurred.",
			timestamp: new Date().toISOString(),
			read: false,
			severity: "error",
		},
	];

	const originalFetch = global.fetch;

	beforeEach(() => {
		vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
		// Mock window.__TAURI_INTERNALS__ absent by default (Browser mode)
		// @ts-ignore
		delete window.__TAURI_INTERNALS__;

		global.fetch = vi.fn().mockImplementation((url: string) => {
			if (url.includes("/notifications")) {
				return Promise.resolve({
					ok: true,
					status: 200,
					json: () => Promise.resolve(mockNotifications),
				} as Response);
			}
			return Promise.resolve({ ok: true, json: () => Promise.resolve({}) } as Response);
		});
	});

	afterEach(() => {
		vi.useRealTimers();
		global.fetch = originalFetch;
		vi.restoreAllMocks();
	});

	it("(a) fetch with env token returns notification array in NotificationCenter", async () => {
		render(
			<NotificationCenter
				isOpen={true}
				onClose={() => {}}
				enableWebSocket={false}
			/>,
		);

		await waitFor(() => {
			expect(screen.getByText("Env Token Notification")).toBeInTheDocument();
		});

		expect(global.fetch).toHaveBeenCalledWith(
			expect.stringContaining("/notifications"),
			expect.objectContaining({
				headers: expect.objectContaining({
					"X-Xavier-Token": expect.any(String),
				}),
			}),
		);
	});

	it("(b) sin Tauri usa polling interval (30s)", async () => {
		render(
			<NotificationCenter
				isOpen={true}
				onClose={() => {}}
				enableWebSocket={false}
			/>,
		);

		await waitFor(() => {
			expect(global.fetch).toHaveBeenCalledTimes(1);
		});

		// Advance timer by 30 seconds
		act(() => {
			vi.advanceTimersByTime(30_000);
		});

		await waitFor(() => {
			expect(global.fetch).toHaveBeenCalledTimes(2);
		});
	});

	it("(c) con Tauri usa listen mock", async () => {
		// Mock Tauri environment
		// @ts-ignore
		window.__TAURI_INTERNALS__ = {};

		const listenMock = vi.fn().mockResolvedValue(() => {});
		vi.doMock("@tauri-apps/api/event", () => ({
			listen: listenMock,
		}));

		render(
			<NotificationCenter
				isOpen={true}
				onClose={() => {}}
				enableWebSocket={false}
			/>,
		);

		await waitFor(() => {
			expect(global.fetch).toHaveBeenCalledTimes(1);
		});

		// Verify 30s polling is NOT scheduled in Tauri mode
		act(() => {
			vi.advanceTimersByTime(30_000);
		});

		expect(global.fetch).toHaveBeenCalledTimes(1);
	});

	it("(d) skeletons mientras loading en NotificationsDropdown", async () => {
		let resolveFetch: (value: any) => void;
		global.fetch = vi.fn().mockImplementation(
			() =>
				new Promise((resolve) => {
					resolveFetch = resolve;
				}),
		);

		const { container } = render(
			<NotificationsDropdown onClose={() => {}} />,
		);

		// Verify skeletons (animate-pulse elements) are visible during loading
		const skeletons = container.querySelectorAll(".animate-pulse");
		expect(skeletons.length).toBeGreaterThan(0);

		// Resolve fetch
		await act(async () => {
			resolveFetch({
				ok: true,
				status: 200,
				json: () => Promise.resolve(mockNotifications),
			});
		});

		await waitFor(() => {
			expect(screen.getByText("Env Token Notification")).toBeInTheDocument();
		});
	});

	it("(e) markRead PATCH con token", async () => {
		render(
			<NotificationCenter
				isOpen={true}
				onClose={() => {}}
				initialNotifications={mockNotifications}
				enableWebSocket={false}
			/>,
		);

		const markReadBtns = screen.getAllByRole("button", { name: /Mark as read/i });
		expect(markReadBtns.length).toBeGreaterThan(0);

		await act(async () => {
			fireEvent.click(markReadBtns[0]);
		});

		await waitFor(() => {
			expect(global.fetch).toHaveBeenCalledWith(
				expect.stringContaining("/notifications/notif-env-1/read"),
				expect.objectContaining({
					method: "PATCH",
					headers: expect.objectContaining({
						"X-Xavier-Token": expect.any(String),
					}),
				}),
			);
		});
	});

	it("(f) markAllRead PATCH con token", async () => {
		render(
			<NotificationCenter
				isOpen={true}
				onClose={() => {}}
				initialNotifications={mockNotifications}
				enableWebSocket={false}
			/>,
		);

		const markAllBtn = screen.getByRole("button", { name: /Mark all read/i });

		await act(async () => {
			fireEvent.click(markAllBtn);
		});

		await waitFor(() => {
			expect(global.fetch).toHaveBeenCalledWith(
				expect.stringContaining("/notifications/read-all"),
				expect.objectContaining({
					method: "PATCH",
					headers: expect.objectContaining({
						"X-Xavier-Token": expect.any(String),
					}),
				}),
			);
		});
	});

	it("(g) no crash transformCallback en browser environment", async () => {
		// Verify rendering in pure browser env without window.__TAURI_INTERNALS__ does not throw transformCallback error
		expect(() => {
			render(
				<NotificationsDropdown onClose={() => {}} />,
			);
		}).not.toThrow();

		await waitFor(() => {
			expect(screen.getByText("Env Token Notification")).toBeInTheDocument();
		});
	});
});
