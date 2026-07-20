import { create } from "zustand";
import { persist } from "zustand/middleware";

interface User {
  id: string;
  email: string;
  name: string;
  role: string;
}

interface AuthState {
  user: User | null;
  accessToken: string | null;
  refreshToken: string | null;
  isAuthenticated: boolean;
  mfaRequired: boolean;
  mfaEmail: string | null;

  setAuth: (user: User, accessToken: string, refreshToken: string) => void;
  setMfaRequired: (email: string) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      user: null,
      accessToken: null,
      refreshToken: null,
      isAuthenticated: false,
      mfaRequired: false,
      mfaEmail: null,

      setAuth: (user, accessToken, refreshToken) =>
        set({
          user,
          accessToken,
          refreshToken,
          isAuthenticated: true,
          mfaRequired: false,
          mfaEmail: null,
        }),

      setMfaRequired: (email) =>
        set({
          mfaRequired: true,
          mfaEmail: email,
          isAuthenticated: false,
        }),

      logout: () =>
        set({
          user: null,
          accessToken: null,
          refreshToken: null,
          isAuthenticated: false,
          mfaRequired: false,
          mfaEmail: null,
        }),
    }),
    {
      name: "xavier-auth-storage",
    },
  ),
);
