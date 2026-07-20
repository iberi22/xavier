import type React from "react";
import { createContext, useContext, useEffect } from "react";
import { create } from "zustand";
import { authClient } from "../api/authClient";
import type { AuthState, User } from "../types";

const useAuthStore = create<AuthState>((set) => ({
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
        token: response.token,
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
    const response = await authClient.register(email, name, password);
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
    } catch (error) {
      set({
        user: null,
        token: null,
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
