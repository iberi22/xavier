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
import { AuthApiError, authClient } from "../api/authClient";
import type { AuthState } from "../types";

// SECURITY: the panel authenticates purely via the /auth/* session (cookie/token flow) —
// never by embedding a raw API token into the built JS bundle. A prior panel deploy shipped
// a build-time master-key env var inlined into the production bundle (a real leak vector,
// since anyone with the built JS could read the master key in plaintext); that env var and
// every read of it have been removed. `token` (used as X-Xavier-Token on panel/* routes) is
// now always sourced from `accessToken`, the operator's raw JWT minted by /auth/login or
// /auth/refresh — see src/types.ts's AuthState.token comment.
const useAuthStore = create<AuthState>((set, get) => ({
	user: null,
	token: null, // X-Xavier-Token for panel/* calls — always the operator's JWT (accessToken) once authenticated
	accessToken: null, // Operator JWT — the raw /auth/* access_token
	refreshToken: null,
	isAuthenticated: false,
	requires2FA: false,
	pendingBackupCodes: null,

	login: async (email, password, totpCode) => {
		try {
			const response = await authClient.login(email, password, totpCode);
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
		await authClient.logout(get().refreshToken);
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
			// /auth/refresh only returns { access_token, refresh_token } — no `user` — so the
			// existing user object is preserved instead of being wiped out by `undefined`.
			const response = await authClient.refresh(get().refreshToken);
			set({
				token: response.access_token ?? null,
				accessToken: response.access_token ?? null,
				refreshToken: response.refresh_token,
				isAuthenticated: true,
			});
		} catch (_error) {
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
