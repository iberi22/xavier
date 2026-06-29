import React, { createContext, useContext, useEffect } from "react";
import { create } from "zustand";
import { authClient } from "../api/authClient";
import type { AuthState, User } from "../types";

const useAuthStore = create<AuthState>((set, get) => ({
  user: null,
  token: null,
  refreshToken: null,
  isAuthenticated: false,
  requires2FA: false,

  login: async (email, password, totpCode) => {
    try {
      const response = await authClient.login(email, password, totpCode);
      set({
        user: response.user,
        token: response.access_token,
        refreshToken: response.refresh_token,
        isAuthenticated: true,
        requires2FA: false,
      });
      localStorage.setItem('xavier_token', response.access_token);
      localStorage.setItem('xavier_refresh', response.refresh_token);
    } catch (error) {
        if (error instanceof Error && error.message.includes("401")) {
            set({ requires2FA: true });
        }
        throw error;
    }
  },

  logout: async () => {
    try {
      await authClient.logout();
    } catch { /* ignore */ }
    localStorage.removeItem('xavier_token');
    localStorage.removeItem('xavier_refresh');
    set({
      user: null,
      token: null,
      refreshToken: null,
      isAuthenticated: false,
      requires2FA: false,
    });
  },

  register: async (email, name, password) => {
    const response = await authClient.register(email, name, password);
    // After successful registration, automatically log in
    const loginResp = await authClient.login(email, password);
    set({
      user: loginResp.user,
      token: loginResp.access_token,
      refreshToken: loginResp.refresh_token,
      isAuthenticated: true,
      requires2FA: false,
    });
    localStorage.setItem('xavier_token', loginResp.access_token);
    localStorage.setItem('xavier_refresh', loginResp.refresh_token);
    return response; // returns RegisterResponse with seed_phrase
  },

  refreshSession: async () => {
    try {
      const response = await authClient.refresh();
      set({
        token: response.access_token,
        refreshToken: response.refresh_token,
        isAuthenticated: true,
      });
      localStorage.setItem('xavier_token', response.access_token);
      localStorage.setItem('xavier_refresh', response.refresh_token);
    } catch (error) {
      // Try restoring from localStorage
      const savedToken = localStorage.getItem('xavier_token');
      if (savedToken) {
        set({ token: savedToken, isAuthenticated: true });
      }
    }
  },

  checkUsers: async () => {
    try {
      const resp = await authClient.checkUsers();
      return resp;
    } catch {
      return { has_users: false, count: 0 };
    }
  },
}));

export { useAuthStore };

const AuthContext = createContext<ReturnType<typeof useAuthStore> | null>(null);

export const AuthProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const refreshSession = useAuthStore((state) => state.refreshSession);

  useEffect(() => {
    // Attempt to refresh session on mount
    void refreshSession();
  }, [refreshSession]);

  return <AuthContext.Provider value={useAuthStore}>{children}</AuthContext.Provider>;
};
