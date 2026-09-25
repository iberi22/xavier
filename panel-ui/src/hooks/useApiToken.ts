/**
 * @file useApiToken.ts
 * @description Centralized browser-safe hook and utility for retrieving the Xavier API token.
 *
 * Sourced exclusively from `useAuthStore` (Zustand state) — i.e. the operator's `/auth/*`
 * session (accessToken minted by /auth/login or /auth/refresh) — falling back to an empty
 * string `""` when unauthenticated.
 *
 * SECURITY: this must never read a build-time env var (e.g. a `VITE_*`-prefixed token) as a
 * fallback. Any `import.meta.env.VITE_*` reference here gets inlined as a plaintext string
 * into the production JS bundle by Vite — a prior panel deploy shipped exactly that
 * (VITE_XAVIER_API_TOKEN baked into the built assets), a real credential-leak vector for
 * anyone who can read the served JS. The panel authenticates purely via the /auth session.
 *
 * This avoids direct calls to IPC commands which fail in browser mode,
 * preventing 401 unauthorized errors when accessing protected endpoints like `/v1/memories` or `/notifications`.
 */

import { useAuthStore } from "../auth/AuthProvider";

/**
 * Custom React hook to get the active API token for X-Xavier-Token requests.
 *
 * @returns {string} The operator's session token (JWT) or empty string when unauthenticated.
 */
export function useApiToken(): string {
  const token = useAuthStore((state) => state.token);
  return token ?? "";
}

/**
 * Synchronous helper function to retrieve the active API token outside React component lifecycles.
 *
 * Useful in event listeners, background fetch timers, or standalone utility functions.
 *
 * @returns {string} The operator's session token (JWT) or empty string when unauthenticated.
 */
export function getApiTokenSync(): string {
  const stateToken = useAuthStore.getState().token;
  return stateToken ?? "";
}

export default useApiToken;
