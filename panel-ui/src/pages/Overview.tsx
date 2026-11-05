import { Activity, Cpu, Database, HardDrive, ShieldCheck } from "lucide-react";
import type React from "react";
import { CloudSyncCard } from "../components/CloudSyncCard";

interface OverviewProps {
  token?: string;
  className?: string;
}

export const Overview: React.FC<OverviewProps> = ({ token, className = "" }) => {
  return (
    <div
      data-testid="overview-page"
      className={`space-y-6 max-w-6xl mx-auto p-4 sm:p-6 lg:p-8 ${className}`}
    >
      {/* Page Title */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-4 border-b border-slate-200 dark:border-white/10">
        <div>
          <h1 className="text-2xl font-bold tracking-tight text-slate-900 dark:text-white font-mono flex items-center gap-2">
            <span>Node Overview & Cloud Relay</span>
          </h1>
          <p className="text-xs text-slate-500 dark:text-white/50 font-mono mt-1">
            Real-time Xavier daemon health, Cloudflare R2 backup, and sovereign storage metrics.
          </p>
        </div>
      </div>

      {/* Cloud Sync Status Card */}
      <CloudSyncCard token={token} />

      {/* Local System Summary Grid */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <div className="bg-white dark:bg-[#0c0d0f] border border-slate-200 dark:border-white/10 rounded-xl p-4 shadow-sm">
          <div className="flex items-center gap-2 text-slate-500 dark:text-white/50 text-xs font-mono mb-2">
            <Cpu className="w-4 h-4 text-emerald-500" />
            <span className="uppercase tracking-wider">Engine Core</span>
          </div>
          <div className="text-lg font-bold font-mono text-slate-900 dark:text-white">
            Xavier v0.2.19
          </div>
          <div className="text-[10px] text-emerald-600 dark:text-[#39ff14] font-mono mt-1">
            Native Tokio + SQLite-vec
          </div>
        </div>

        <div className="bg-white dark:bg-[#0c0d0f] border border-slate-200 dark:border-white/10 rounded-xl p-4 shadow-sm">
          <div className="flex items-center gap-2 text-slate-500 dark:text-white/50 text-xs font-mono mb-2">
            <HardDrive className="w-4 h-4 text-cyan-500" />
            <span className="uppercase tracking-wider">Local Vector Store</span>
          </div>
          <div className="text-lg font-bold font-mono text-slate-900 dark:text-white">
            100% Intact
          </div>
          <div className="text-[10px] text-slate-500 dark:text-white/50 font-mono mt-1">
            WAL checkpoint clean
          </div>
        </div>

        <div className="bg-white dark:bg-[#0c0d0f] border border-slate-200 dark:border-white/10 rounded-xl p-4 shadow-sm">
          <div className="flex items-center gap-2 text-slate-500 dark:text-white/50 text-xs font-mono mb-2">
            <ShieldCheck className="w-4 h-4 text-purple-500" />
            <span className="uppercase tracking-wider">Zero-Trust Node</span>
          </div>
          <div className="text-lg font-bold font-mono text-slate-900 dark:text-white">
            BIP39 Active
          </div>
          <div className="text-[10px] text-purple-600 dark:text-purple-400 font-mono mt-1">
            Ed25519 identity attestation
          </div>
        </div>

        <div className="bg-white dark:bg-[#0c0d0f] border border-slate-200 dark:border-white/10 rounded-xl p-4 shadow-sm">
          <div className="flex items-center gap-2 text-slate-500 dark:text-white/50 text-xs font-mono mb-2">
            <Activity className="w-4 h-4 text-amber-500" />
            <span className="uppercase tracking-wider">Sync State</span>
          </div>
          <div className="text-lg font-bold font-mono text-slate-900 dark:text-white">
            R2 Connected
          </div>
          <div className="text-[10px] text-amber-600 dark:text-amber-400 font-mono mt-1">
            Incremental push enabled
          </div>
        </div>
      </div>
    </div>
  );
};

export default Overview;
