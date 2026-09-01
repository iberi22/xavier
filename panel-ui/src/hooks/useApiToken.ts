/// <reference types="vite/client" />
/**
 * @file useApiToken.ts
 * @description Centralized browser-safe hook and utility for retrieving the Xavier API token.
 *
 * Prefers token stored in `useAuthStore` (Zustand state), falling back to
 * `import.meta.env.VITE_XAVIER_API_TOKEN` and finally an empty string `""`.
 *
 * This avoids direct calls to IPC commands which fail in browser mode,
 * preventing 401 unauthorized errors when accessing protected endpoints like `/v1/memories` or `/notifications`.
 */

import { useAuthStore } from "../auth/AuthProvider";

/**
 * Custom React hook to get the active API token for X-Xavier-Token requests.
 *
 * @returns {string} The active master API token or empty string.
 */
export function useApiToken(): string {
  const token = useAuthStore((state) => state.token);
  return (
    token ??
    (import.meta.env.VITE_XAVIER_API_TOKEN as string | undefined) ??
    ""
  );
}

/**
 * Synchronous helper function to retrieve the active API token outside React component lifecycles.
 *
 * Useful in event listeners, background fetch timers, or standalone utility functions.
 *
 * @returns {string} The active master API token or empty string.
 */
export function getApiTokenSync(): string {
  const stateToken = useAuthStore.getState().token;
  return (
    stateToken ??
    (import.meta.env.VITE_XAVIER_API_TOKEN as string | undefined) ??
    ""
  );
}

export default useApiToken;
