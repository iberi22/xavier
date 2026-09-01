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
import { authClient } from "../api/authClient";
import type { AuthState } from "../types";

// The panel uses the master API key (VITE_XAVIER_API_TOKEN) for X-Xavier-Token panel routes.
// The operator JWT is stored separately in refreshToken for session management.
const API_TOKEN =
	(import.meta.env.VITE_XAVIER_API_TOKEN as string | undefined) ?? null;

const useAuthStore = create<AuthState>((set) => ({
	user: null,
	token: API_TOKEN, // Master API key — used as X-Xavier-Token in panel/* calls
	refreshToken: null,
	isAuthenticated: false,
	requires2FA: false,

	login: async (email, password, totpCode) => {
		try {
			const response = await authClient.login(email, password, totpCode);
			// access_token is the JWT; use it only if no env token is configured.
			const jwtToken =
				(response as unknown as { access_token?: string }).access_token ??
				response.token;
			set({
				user: response.user,
				token: API_TOKEN ?? jwtToken ?? null,
				refreshToken: response.refresh_token,
				isAuthenticated: true,
				requires2FA: false,
			});
		} catch (error) {
			if (error instanceof Error && error.message.includes("2FA")) {
				set({ requires2FA: true });
			}
			throw error;
		}
	},

	logout: async () => {
		await authClient.logout();
		set({
			user: null,
			token: null,
			refreshToken: null,
			isAuthenticated: false,
			requires2FA: false,
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
			const response = await authClient.refresh();
			set({
				user: response.user,
				token: response.token,
				refreshToken: response.refresh_token,
				isAuthenticated: true,
			});
		} catch (_error) {
			set({
				user: null,
				token: API_TOKEN,
				refreshToken: null,
				isAuthenticated: false,
			});
		}
	},
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
