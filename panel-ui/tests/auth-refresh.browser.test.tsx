import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { authClient } from "../src/api/authClient";
import { AuthProvider, useAuthStore } from "../src/auth/AuthProvider";

vi.mock("../src/api/authClient", () => ({
	authClient: {
		login: vi.fn(),
		logout: vi.fn(),
		register: vi.fn(),
		refresh: vi.fn(),
	},
	AuthApiError: class AuthApiError extends Error {
		code?: string;
		constructor(message: string, code?: string) {
			super(message);
			this.name = "AuthApiError";
			this.code = code;
		}
	},
}));

// SECURITY: `token` (X-Xavier-Token) is sourced purely from the operator's /auth session
// (accessToken) — never from a build-time env var. See src/auth/AuthProvider.tsx.
describe("AuthProvider Refresh & /auth session token sourcing", () => {
	const existingUser = { id: "1", email: "user@example.com", role: "admin" };

	beforeEach(() => {
		vi.clearAllMocks();
		useAuthStore.setState({
			user: existingUser,
			token: "old-jwt-token",
			accessToken: "old-jwt-token",
			refreshToken: "old-refresh-token",
			isAuthenticated: false,
			requires2FA: false,
			pendingBackupCodes: null,
		});
	});

	// /auth/refresh (refresh_handler, src/auth2/mod.rs) returns only
	// `{ access_token, refresh_token }` — never a `user` field — so a successful
	// refresh must preserve whatever `user` was already in the store.
	test("(a) refresh 200 -> isAuthenticated true, token/accessToken/refreshToken updated, user preserved", async () => {
		(authClient.refresh as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
			access_token: "new-jwt-token",
			refresh_token: "new-refresh-token",
		});

		await useAuthStore.getState().refreshSession();

		const state = useAuthStore.getState();
		expect(state.user).toEqual(existingUser);
		expect(state.token).toBe("new-jwt-token");
		expect(state.accessToken).toBe("new-jwt-token");
		expect(state.refreshToken).toBe("new-refresh-token");
		expect(state.isAuthenticated).toBe(true);
	});

	test("(b) refresh 400 -> token/accessToken/refreshToken cleared, isAuthenticated false", async () => {
		(authClient.refresh as ReturnType<typeof vi.fn>).mockRejectedValueOnce(
			new Error("400 Bad Request: Invalid refresh token"),
		);

		await useAuthStore.getState().refreshSession();

		const state = useAuthStore.getState();
		expect(state.user).toBeNull();
		expect(state.token).toBeNull();
		expect(state.accessToken).toBeNull();
		expect(state.refreshToken).toBeNull();
		expect(state.isAuthenticated).toBe(false);
	});

	test("(c) refresh network error -> token cleared, isAuthenticated false", async () => {
		(authClient.refresh as ReturnType<typeof vi.fn>).mockRejectedValueOnce(
			new Error("Network Error"),
		);

		await useAuthStore.getState().refreshSession();

		const state = useAuthStore.getState();
		expect(state.user).toBeNull();
		expect(state.token).toBeNull();
		expect(state.refreshToken).toBeNull();
		expect(state.isAuthenticated).toBe(false);
	});

	// /auth/login (login_handler) returns `access_token`, not `token` — the client
	// type (LoginResponse in authClient.ts) must match this exactly.
	test("(d) login -> token is set to the /auth access_token", async () => {
		const mockUser = {
			id: "2",
			email: "operator@example.com",
			role: "operator",
		};
		(authClient.login as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
			user: mockUser,
			access_token: "jwt-token-123",
			refresh_token: "refresh-token-123",
		});

		await useAuthStore.getState().login("operator@example.com", "password");

		const state = useAuthStore.getState();
		expect(state.user).toEqual(mockUser);
		expect(state.token).toBe("jwt-token-123");
		expect(state.accessToken).toBe("jwt-token-123");
		expect(state.refreshToken).toBe("refresh-token-123");
		expect(state.isAuthenticated).toBe(true);
	});

	test("(e) AuthProvider calls refreshSession only once on mount without infinite loop", async () => {
		(authClient.refresh as ReturnType<typeof vi.fn>).mockRejectedValue(
			new Error("400 Invalid refresh token"),
		);

		render(
			<AuthProvider>
				<div data-testid="child">Child Component</div>
			</AuthProvider>,
		);

		await waitFor(() => {
			expect(authClient.refresh).toHaveBeenCalledTimes(1);
		});

		// Wait extra tick to ensure no secondary trigger loops occur
		await new Promise((resolve) => setTimeout(resolve, 50));
		expect(authClient.refresh).toHaveBeenCalledTimes(1);
	});
});
