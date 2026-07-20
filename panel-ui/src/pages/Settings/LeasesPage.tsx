import { Key, RefreshCw, ShieldAlert } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import { ApiClient } from "../../api/client";
import LeaseCard from "../../components/LeaseCard";
import type { SecretLease } from "../../types";

interface LeasesPageProps {
  token: string;
}

export default function LeasesPage({ token }: LeasesPageProps) {
  const [leases, setLeases] = useState<SecretLease[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const apiClient = new ApiClient(token);

  const fetchLeases = useCallback(async () => {
    try {
      setLoading(true);
      const data = await apiClient.getLeases();
      setLeases(data);
      setError(null);
    } catch (err) {
      setError("Failed to fetch active leases");
      console.error(err);
    } finally {
      setLoading(false);
    }
  }, [token]);

  useEffect(() => {
    fetchLeases();
    const interval = setInterval(fetchLeases, 30000);
    return () => clearInterval(interval);
  }, [fetchLeases]);

  const handleRevoke = async (leaseToken: string) => {
    try {
      await apiClient.revokeLease(leaseToken);
      setLeases((prev) => prev.filter((l) => l.token !== leaseToken));
    } catch (err) {
      console.error("Failed to revoke lease:", err);
    }
  };

  if (loading && leases.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-64 text-white/40">
        <RefreshCw className="w-8 h-8 mb-4 animate-spin opacity-20" />
        <p className="text-sm font-light tracking-widest uppercase">
          Scanning for active leases...
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-medium text-white/80">
            Active Secret Leases
          </h3>
          <p className="text-[10px] text-white/30 mt-0.5">
            Temporary keys currently lent to agents and external services.
          </p>
        </div>
        <button
          onClick={fetchLeases}
          className="p-2 text-white/20 hover:text-[#39ff14] hover:bg-[#39ff14]/10 rounded-lg transition-all"
          title="Refresh Leases"
        >
          <RefreshCw className={`w-4 h-4 ${loading ? "animate-spin" : ""}`} />
        </button>
      </div>

      {error && (
        <div className="p-4 rounded-xl bg-red-500/10 border border-red-500/20 flex items-center gap-3 text-red-400 text-xs">
          <ShieldAlert className="w-4 h-4" />
          {error}
        </div>
      )}

      {leases.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-12 px-4 rounded-2xl bg-white/[0.01] border border-dashed border-white/5">
          <Key className="w-12 h-12 text-white/5 mb-4" />
          <p className="text-white/30 text-sm text-center">
            No active secret leases found.
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <AnimatePresence mode="popLayout">
            {leases.map((lease) => (
              <motion.div
                key={lease.token}
                layout
                initial={{ opacity: 0, scale: 0.9 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.9 }}
                transition={{ duration: 0.2 }}
              >
                <LeaseCard lease={lease} onRevoke={handleRevoke} />
              </motion.div>
            ))}
          </AnimatePresence>
        </div>
      )}
    </div>
  );
}
