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

// The panel uses the master API key (VITE_XAVIER_API_TOKEN) for X-Xavier-Token panel routes.
// The operator's raw JWT always lives in `accessToken`; `refreshToken` holds the opaque
// rotation token used only to mint new access tokens via /auth/refresh.
const API_TOKEN =
	(import.meta.env.VITE_XAVIER_API_TOKEN as string | undefined) ?? null;

const useAuthStore = create<AuthState>((set, get) => ({
	user: null,
	token: API_TOKEN, // Master API key — used as X-Xavier-Token in panel/* calls
	accessToken: null, // Operator JWT — always the raw /auth/* access_token, independent of API_TOKEN
	refreshToken: null,
	isAuthenticated: false,
	requires2FA: false,
	pendingBackupCodes: null,

	login: async (email, password, totpCode) => {
		try {
			const response = await authClient.login(email, password, totpCode);
			set({
				user: response.user,
				// Panel/* routes prefer the master API key when configured; auth-protected
				// routes (2fa/setup, 2fa/verify) always use `accessToken` below.
				token: API_TOKEN ?? response.access_token ?? null,
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
			token: API_TOKEN,
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
				token: API_TOKEN ?? response.access_token ?? null,
				accessToken: response.access_token ?? null,
				refreshToken: response.refresh_token,
				isAuthenticated: true,
			});
		} catch (_error) {
			set({
				user: null,
				token: API_TOKEN,
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
