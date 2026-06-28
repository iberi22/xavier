import { useEffect } from "react";
import { useAuthStore } from "../auth/AuthProvider";

export const useSession = () => {
  const refreshSession = useAuthStore((state) => state.refreshSession);
  const isAuthenticated = useAuthStore((state) => state.isAuthenticated);

  useEffect(() => {
    // Session refresh logic could be more complex here, e.g., interval-based
    const interval = setInterval(() => {
      if (isAuthenticated) {
        void refreshSession();
      }
    }, 15 * 60 * 1000); // Refresh every 15 minutes

    return () => clearInterval(interval);
  }, [isAuthenticated, refreshSession]);

  return {
    refreshSession,
    isAuthenticated,
  };
};
