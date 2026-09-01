import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import App from "../src/App";
import { useAuthStore } from "../src/auth/AuthProvider";

// Mock child components that might make real fetch calls or render heavy canvas/animations
vi.mock("../src/components/ParticleBackground", () => ({
	default: () => <div data-testid="particle-bg" />,
}));

vi.mock("../src/components/TopStatusBar", () => ({
	default: () => <div data-testid="top-status-bar" />,
}));

vi.mock("../src/auth/AuthProvider", () => ({
	useAuthStore: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
	invoke: vi.fn(),
}));

describe("App Native & HTTP Config Check", () => {
	const originalWindowTauri = (window as any).__TAURI_INTERNALS__;

	beforeEach(() => {
		vi.clearAllMocks();
		(useAuthStore as any).mockReturnValue({
			token: "mock-token",
			user: { id: "1", name: "Test User" },
			isAuthenticated: true,
			requires2FA: false,
		});
		delete (window as any).__TAURI_INTERNALS__;
		localStorage.setItem("xavier_onboarding_completed", "true");
	});

	afterEach(() => {
		if (originalWindowTauri !== undefined) {
			(window as any).__TAURI_INTERNALS__ = originalWindowTauri;
		} else {
			delete (window as any).__TAURI_INTERNALS__;
		}
	});

	test("Browser mode: non-Tauri environment with fetch 200 OK sets hasConfig to true", async () => {
		const fetchSpy = vi
			.spyOn(globalThis, "fetch")
			.mockImplementation(async (url) => {
				const urlStr = String(url);
				if (urlStr.includes("/v1/config/providers")) {
					return new Response(
						JSON.stringify({ providers: [{ provider: "openai" }] }),
						{
							status: 200,
							headers: { "Content-Type": "application/json" },
						},
					);
				}
				if (urlStr.includes("/health")) {
					return new Response(JSON.stringify({ status: "ok" }), {
						status: 200,
					});
				}
				if (urlStr.includes("/panel/api/threads")) {
					return new Response(JSON.stringify([]), { status: 200 });
				}
				if (urlStr.includes("/panel/api/bookmarks")) {
					return new Response(JSON.stringify([]), { status: 200 });
				}
				return new Response(JSON.stringify({}), { status: 200 });
			});

		render(<App />);

		await waitFor(() => {
			expect(fetchSpy).toHaveBeenCalledWith(
				expect.stringContaining("/v1/config/providers"),
				expect.objectContaining({
					headers: { "X-Xavier-Token": "mock-token" },
				}),
			);
		});

		expect(screen.queryByText("Xavier Offline")).not.toBeInTheDocument();
	});

	test("Browser mode: non-Tauri environment with fetch 404 sets hasConfig to true (Ollama fallback)", async () => {
		const fetchSpy = vi
			.spyOn(globalThis, "fetch")
			.mockImplementation(async (url) => {
				const urlStr = String(url);
				if (urlStr.includes("/v1/config/providers")) {
					return new Response("Not Found", { status: 404 });
				}
				if (urlStr.includes("/health")) {
					return new Response(JSON.stringify({ status: "ok" }), {
						status: 200,
					});
				}
				if (urlStr.includes("/panel/api/threads")) {
					return new Response(JSON.stringify([]), { status: 200 });
				}
				if (urlStr.includes("/panel/api/bookmarks")) {
					return new Response(JSON.stringify([]), { status: 200 });
				}
				return new Response(JSON.stringify({}), { status: 200 });
			});

		render(<App />);

		await waitFor(() => {
			expect(fetchSpy).toHaveBeenCalledWith(
				expect.stringContaining("/v1/config/providers"),
				expect.anything(),
			);
		});

		expect(screen.queryByText("Xavier Offline")).not.toBeInTheDocument();
	});

	test("Tauri mode: __TAURI_INTERNALS__ present calls dynamic invoke('get_current_config_state')", async () => {
		(window as any).__TAURI_INTERNALS__ = {};
		const { invoke } = await import("@tauri-apps/api/core");
		(invoke as any).mockResolvedValue({
			has_openai: true,
			has_gemini: false,
		});

		const fetchSpy = vi
			.spyOn(globalThis, "fetch")
			.mockImplementation(async (url) => {
				const urlStr = String(url);
				if (urlStr.includes("/health")) {
					return new Response(JSON.stringify({ status: "ok" }), {
						status: 200,
					});
				}
				if (urlStr.includes("/panel/api/threads")) {
					return new Response(JSON.stringify([]), { status: 200 });
				}
				if (urlStr.includes("/panel/api/bookmarks")) {
					return new Response(JSON.stringify([]), { status: 200 });
				}
				return new Response(JSON.stringify({}), { status: 200 });
			});

		render(<App />);

		await waitFor(() => {
			expect(invoke).toHaveBeenCalledWith("get_current_config_state");
		});

		const providerCalls = fetchSpy.mock.calls.filter(([url]) =>
			String(url).includes("/v1/config/providers"),
		);
		expect(providerCalls.length).toBe(0);
	});

	test("Browser mode: handles missing static @tauri-apps/api/core safely without TypeError", async () => {
		delete (window as any).__TAURI_INTERNALS__;

		const fetchSpy = vi
			.spyOn(globalThis, "fetch")
			.mockImplementation(async (url) => {
				const urlStr = String(url);
				if (urlStr.includes("/health")) {
					return new Response(JSON.stringify({ status: "ok" }), {
						status: 200,
					});
				}
				if (urlStr.includes("/v1/config/providers")) {
					return new Response(JSON.stringify({ providers: [] }), {
						status: 200,
					});
				}
				return new Response(JSON.stringify([]), { status: 200 });
			});

		expect(() => render(<App />)).not.toThrow();

		await waitFor(() => {
			expect(fetchSpy).toHaveBeenCalledWith(
				expect.stringContaining("/v1/config/providers"),
				expect.anything(),
			);
		});
	});
});
