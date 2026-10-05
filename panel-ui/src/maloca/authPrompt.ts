// Auth-required state for the Maloca panel. When a call returns 401 the API layer
// asks for a token here; the AuthPrompt component renders the form and resolves it.
// The token is only ever written to localStorage["auth_token"] and later sent as the
// X-Xavier-Token header to the same origin. It is never logged.

export const AUTH_TOKEN_KEY = "auth_token";

type Listener = (open: boolean) => void;

let pending: Promise<string | null> | null = null;
let settle: ((token: string | null) => void) | null = null;
const listeners = new Set<Listener>();

function notify(open: boolean) {
  for (const l of listeners) l(open);
}

/** Resolves with the saved token, or null if the user dismissed the prompt. Shared across concurrent 401s. */
export function requestMalocaToken(): Promise<string | null> {
  if (!pending) {
    pending = new Promise<string | null>((resolve) => {
      settle = resolve;
    });
    notify(true);
  }
  return pending;
}

function close(token: string | null) {
  const resolve = settle;
  pending = null;
  settle = null;
  notify(false);
  resolve?.(token);
}

export function submitMalocaToken(raw: string): boolean {
  const token = raw.trim();
  if (!token) return false;
  localStorage.setItem(AUTH_TOKEN_KEY, token);
  close(token);
  return true;
}

export function cancelMalocaToken() {
  close(null);
}

export function isMalocaAuthPending(): boolean {
  return pending !== null;
}

export function subscribeMalocaAuth(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
