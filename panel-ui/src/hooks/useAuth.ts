import { useAuthStore } from "../auth/AuthProvider";

export const useAuth = () => {
  const user = useAuthStore((state) => state.user);
  const isAuthenticated = useAuthStore((state) => state.isAuthenticated);
  const requires2FA = useAuthStore((state) => state.requires2FA);
  const login = useAuthStore((state) => state?.login);
  const logout = useAuthStore((state) => state?.logout);
  const register = useAuthStore((state) => state?.register);

  return {
    user,
    isAuthenticated,
    requires2FA,
    login,
    logout,
    register,
  };
};
