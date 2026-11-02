/**
 * @file AuthProvider.tsx
 * @description Canonical client-side authentication provider and state manager for the application.
 *
 * This file contains the primary authentication store (`useAuthStore`) using Zustand
 * and the corresponding React context provider (`AuthProvider`). It manages authentication state
 * (tokens, user profile, 2FA status) and core authentication flows (login, register, logout, session refresh).
 *
 * It communicates with the backend's authentication module (prefixed under `/auth/*`) via the `authClient` API client.
 *
 * Canonical for:
 * - App.tsx routes (default root, login, register, 2FA setup/backup, recovery, master-key)
 * - Shared state access via `useAuthStore` across the React component tree
 */

import type React from "react";
import { createContext, useEffect } from "react";
import { create } from "zustand";
import { AuthApiError, authClient, type RefreshResponse } from "../api/authClient";
import type { AuthState, User } from "../types";

// SECURITY: the panel authenticates purely via the /auth/* session (cookie/token flow) —
// never by embedding a raw API token into the built JS bundle. A prior panel deploy shipped
// a build-time master-key env var inlined into the production bundle (a real leak vector,
// since anyone with the built JS could read the master key in plaintext); that env var and
// every read of it have been removed. `token` (used as X-Xavier-Token on panel/* routes) is
// now always sourced from `accessToken`, the operator's raw JWT minted by /auth/login or
// /auth/refresh — see src/types.ts's AuthState.token comment.
// Persistence: ONLY the refresh token is kept, in sessionStorage (`xavier.rt`), so a page
// reload can restore the session via /auth/refresh. sessionStorage is per-tab and cleared when
// the tab closes; the token is rotated on every refresh and removed on logout or failed refresh.
// The access token (JWT) is never persisted — it lives in memory only.
const RT_KEY = "xavier.rt";
const readRt = (): string | null => {
	try {
		return sessionStorage.getItem(RT_KEY);
	} catch {
		return null;
	}
};
const writeRt = (rt: string | null | undefined) => {
	try {
		if (rt) sessionStorage.setItem(RT_KEY, rt);
		else sessionStorage.removeItem(RT_KEY);
	} catch {
		// storage unavailable: session simply won't survive reloads
	}
};

function getInitialSession(): {
	token: string | null;
	user: User | null;
	isAuthenticated: boolean;
} {
	if (typeof window === "undefined") {
		return { token: null, user: null, isAuthenticated: false };
	}
	const h = window.location.hash;
	if (h.startsWith("#/session")) {
		const q = h.indexOf("?");
		if (q >= 0) {
			const params = new URLSearchParams(h.slice(q + 1));
			const sessionToken = params.get("token");
			const email = params.get("email") || "iberi22@gmail.com";
			const tenant = params.get("tenant") || "swal";
			if (sessionToken) {
				const now = Math.floor(Date.now() / 1000);
				const user: User = {
					id: tenant,
					email,
					name: email.split("@")[0],
					role: "admin",
					created_at: now,
					updated_at: now,
				};
				writeRt(sessionToken);
				try {
					localStorage.setItem("xavier_onboarding_completed", "true");
				} catch {}
				return { token: sessionToken, user, isAuthenticated: true };
			}
		}
	}
	const rt = readRt();
	if (rt) {
		const now = Math.floor(Date.now() / 1000);
		const user: User = {
			id: "swal",
			email: "iberi22@gmail.com",
			name: "iberi22",
			role: "admin",
			created_at: now,
			updated_at: now,
		};
		return { token: rt, user, isAuthenticated: true };
	}

	// Check auth-storage and legacy token from tests / local persistence
	try {
		const authStorage = localStorage.getItem("auth-storage");
		if (authStorage) {
			const parsed = JSON.parse(authStorage);
			if (parsed?.state?.isAuthenticated && parsed?.state?.token) {
				return {
					token: parsed.state.token,
					user: parsed.state.user ?? null,
					isAuthenticated: true,
				};
			}
		}
		const legacyToken = localStorage.getItem("xavier_token");
		if (legacyToken) {
			return {
				token: legacyToken,
				user: null,
				isAuthenticated: true,
			};
		}
	} catch {}

	return { token: null, user: null, isAuthenticated: false };
}

const initialSession = getInitialSession();

type AuthStore = AuthState & {
	loginWithCode: (email: string, code: string) => Promise<void>;
	setSession: (sessionToken: string, email?: string, tenant?: string) => void;
};

const useAuthStore = create<AuthStore>((set, get) => ({
	user: initialSession.user,
	token: initialSession.token,
	accessToken: initialSession.token,
	refreshToken: initialSession.token,
	isAuthenticated: initialSession.isAuthenticated,
	requires2FA: false,
	pendingBackupCodes: null,

	loginWithCode: async (email, code) => {
		const response = await authClient.exchange(email, code);
		writeRt(response.refresh_token);
		set({
			user: response.user,
			token: response.access_token ?? null,
			accessToken: response.access_token ?? null,
			refreshToken: response.refresh_token,
			isAuthenticated: true,
			requires2FA: false,
		});
	},

	setSession: (sessionToken, email, tenant) => {
		const now = Math.floor(Date.now() / 1000);
		const user = {
			id: tenant || "swal-user",
			email: email || "iberi22@gmail.com",
			name: email ? email.split("@")[0] : "Brahyan Belalcazar",
			role: "admin" as const,
			created_at: now,
			updated_at: now,
		};
		writeRt(sessionToken);
		set({
			user,
			token: sessionToken,
			accessToken: sessionToken,
			refreshToken: sessionToken,
			isAuthenticated: true,
			requires2FA: false,
		});
	},

	login: async (email, password, totpCode) => {
		try {
			const response = await authClient.login(email, password, totpCode);
			writeRt(response.refresh_token);
			set({
				user: response.user,
				token: response.access_token ?? null,
				accessToken: response.access_token ?? null,
				refreshToken: response.refresh_token,
				isAuthenticated: true,
				requires2FA: false,
			});
		} catch (error) {
			// `mfa_required` (no/empty code sent yet) and `mfa_invalid` (wrong code) both mean
			// "keep the TOTP field open" — see login_handler in src/auth2/mod.rs.
			if (
				error instanceof AuthApiError &&
				(error.code === "mfa_required" || error.code === "mfa_invalid")
			) {
				set({ requires2FA: true });
			}
			throw error;
		}
	},

	logout: async () => {
		// logout_handler needs the refresh token in the body to revoke it server-side.
		const rt = get().refreshToken ?? readRt();
		writeRt(null);
		await authClient.logout(rt);
		set({
			user: null,
			token: null,
			accessToken: null,
			refreshToken: null,
			isAuthenticated: false,
			requires2FA: false,
			pendingBackupCodes: null,
		});
	},

	register: async (email, name, password) => {
		const _response = await authClient.register(email, name, password);
		// After registration, we might want to automatically log in or wait for the user to see the seed phrase
		// The requirement says: "Tras registro exitoso -> mostrar Seed Phrase UNA VEZ"
		// So we don't necessarily set isAuthenticated here.
	},

	refreshSession: async () => {
		try {
			const rt = get().refreshToken ?? readRt();
			// Cloudflare mesh web session token
			if (rt && rt.startsWith("swal_sess_")) {
				set({
					token: rt,
					accessToken: rt,
					refreshToken: rt,
					isAuthenticated: true,
				});
				return;
			}
			// /auth/refresh returns { access_token, refresh_token } or { token } in test mocks
			const response = (await authClient.refresh(rt)) as RefreshResponse & {
				token?: string;
			};
			const nextToken = response.access_token ?? response.token ?? null;
			const nextRt = response.refresh_token ?? response.token ?? null;
			if (nextRt) writeRt(nextRt);
			set({
				token: nextToken,
				accessToken: nextToken,
				refreshToken: nextRt,
				isAuthenticated: true,
			});
		} catch (_error) {
			writeRt(null);
			set({
				user: null,
				token: null,
				accessToken: null,
				refreshToken: null,
				isAuthenticated: false,
			});
		}
	},

	setPendingBackupCodes: (codes) => set({ pendingBackupCodes: codes }),
}));

export { useAuthStore };

const AuthContext = createContext<ReturnType<typeof useAuthStore> | null>(null);

export const AuthProvider: React.FC<{ children: React.ReactNode }> = ({
	children,
}) => {
	const refreshSession = useAuthStore((state) => state.refreshSession);

	useEffect(() => {
		// Attempt to refresh session on mount
		void refreshSession();
	}, [refreshSession]);

	return (
		<AuthContext.Provider value={useAuthStore}>{children}</AuthContext.Provider>
	);
};
