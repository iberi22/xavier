import {
  Activity,
  Check,
  Database,
  HardDrive,
  Layers,
  Radio,
  RefreshCw,
  ShieldCheck,
  Users,
  Wifi,
  Zap,
} from "lucide-react";
import React, { useEffect, useMemo, useState } from "react";

export interface DataNodeMetrics {
  connectedPeers: number;
  totalSyncedRecords: number;
  bandwidthUsageMbps: number;
  latencyMs: number;
  lastSyncTimestamp: string;
}

export interface DataNodeDashboardProps {
  initialOptIn?: boolean;
  initialQuotaMb?: number;
  initialLocalDbSizeMb?: number;
  initialMetrics?: DataNodeMetrics;
  onOptInChange?: (enabled: boolean) => void;
  onQuotaChange?: (quotaMb: number) => void;
  onSyncTrigger?: () => Promise<void> | void;
}

const STORAGE_KEYS = {
  OPT_IN: "maloca_datanode_opt_in",
  QUOTA: "maloca_datanode_quota_mb",
};

const DEFAULT_METRICS: DataNodeMetrics = {
  connectedPeers: 12,
  totalSyncedRecords: 48250,
  bandwidthUsageMbps: 1.4,
  latencyMs: 38,
  lastSyncTimestamp: "Just now",
};

export function DataNodeDashboard({
  initialOptIn,
  initialQuotaMb,
  initialLocalDbSizeMb = 1840, // Default ~1.84 GB
  initialMetrics,
  onOptInChange,
  onQuotaChange,
  onSyncTrigger,
}: DataNodeDashboardProps) {
  // Initialize Opt-In from prop or localStorage (defaults to false if neither present)
  const [optIn, setOptIn] = useState<boolean>(() => {
    if (initialOptIn !== undefined) return initialOptIn;
    try {
      const stored = localStorage.getItem(STORAGE_KEYS.OPT_IN);
      return stored !== null ? JSON.parse(stored) === true : false;
    } catch {
      return false;
    }
  });

  // Initialize Storage Quota (in MB) from prop or localStorage (defaults to 5000 MB ~ 5GB)
  const [quotaMb, setQuotaMb] = useState<number>(() => {
    if (initialQuotaMb !== undefined) return initialQuotaMb;
    try {
      const stored = localStorage.getItem(STORAGE_KEYS.QUOTA);
      return stored !== null ? Number.parseInt(stored, 10) : 5000;
    } catch {
      return 5000;
    }
  });

  const [localDbSizeMb] = useState<number>(initialLocalDbSizeMb);
  const [metrics, setMetrics] = useState<DataNodeMetrics>(
    initialMetrics || DEFAULT_METRICS
  );
  const [isSyncing, setIsSyncing] = useState<boolean>(false);
  const [isRefreshing, setIsRefreshing] = useState<boolean>(false);
  const [lastRefreshed, setLastRefreshed] = useState<string>("Just now");

  // Save Opt-In state to localStorage
  const handleToggleOptIn = () => {
    const nextVal = !optIn;
    setOptIn(nextVal);
    try {
      localStorage.setItem(STORAGE_KEYS.OPT_IN, JSON.stringify(nextVal));
    } catch {
      // ignore storage errors
    }
    onOptInChange?.(nextVal);
  };

  // Save Quota state to localStorage
  const handleQuotaChange = (newQuota: number) => {
    const clampedQuota = Math.max(500, Math.min(50000, newQuota));
    setQuotaMb(clampedQuota);
    try {
      localStorage.setItem(STORAGE_KEYS.QUOTA, clampedQuota.toString());
    } catch {
      // ignore storage errors
    }
    onQuotaChange?.(clampedQuota);
  };

  const handleSyncTrigger = async () => {
    if (!optIn || isSyncing) return;
    setIsSyncing(true);
    try {
      if (onSyncTrigger) {
        await onSyncTrigger();
      } else {
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
      setMetrics((prev) => ({
        ...prev,
        totalSyncedRecords: prev.totalSyncedRecords + Math.floor(Math.random() * 50) + 1,
        lastSyncTimestamp: "Just now",
      }));
    } finally {
      setIsSyncing(false);
    }
  };

  const handleRefreshStatus = async () => {
    setIsRefreshing(true);
    try {
      await new Promise((resolve) => setTimeout(resolve, 500));
      setLastRefreshed("Just now");
    } finally {
      setIsRefreshing(false);
    }
  };

  // Derived usage calculation
  const usagePercentage = useMemo(() => {
    if (quotaMb <= 0) return 0;
    const pct = (localDbSizeMb / quotaMb) * 100;
    return Math.min(100, Math.round(pct * 10) / 10);
  }, [localDbSizeMb, quotaMb]);

  const networkStatus = useMemo(() => {
    if (!optIn) return { label: "Paused", color: "bg-amber-500/20 text-amber-400 border-amber-500/30", dot: "bg-amber-400" };
    return { label: "Active", color: "bg-emerald-500/20 text-emerald-400 border-emerald-500/30", dot: "bg-emerald-400" };
  }, [optIn]);

  const formattedDbSize = (localDbSizeMb / 1024).toFixed(2);
  const formattedQuota = (quotaMb / 1024).toFixed(2);

  return (
    <div className="bg-[#050505]/60 border border-white/10 rounded-2xl p-6 shadow-xl backdrop-blur-md">
      {/* Header section */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-6 border-b border-white/10">
        <div className="flex items-center gap-4">
          <div className="p-3 bg-emerald-500/10 border border-emerald-500/20 rounded-xl text-emerald-400">
            <Database className="w-6 h-6" aria-hidden="true" />
          </div>
          <div>
            <div className="flex items-center gap-3">
              <h2 className="text-xl font-bold tracking-tight text-white">
                Maloca Data Node
              </h2>
              <span
                className={`px-2.5 py-0.5 text-xs font-semibold rounded-full border flex items-center gap-1.5 ${networkStatus.color}`}
              >
                <span className={`w-2 h-2 rounded-full ${networkStatus.dot} animate-pulse`} />
                {networkStatus.label}
              </span>
            </div>
            <p className="text-xs text-white/50 mt-1">
              Participate in Maloca consensus & distributed SQLite data sync network.
            </p>
          </div>
        </div>

        {/* Master Opt-In Toggle */}
        <div className="flex items-center gap-3 bg-white/5 px-4 py-2.5 rounded-xl border border-white/5 self-start sm:self-auto">
          <label htmlFor="datanode-optin-toggle" className="text-xs font-medium text-white/80 select-none">
            {optIn ? "Data Node Enabled" : "Opt-In Data Node"}
          </label>
          <button
            id="datanode-optin-toggle"
            type="button"
            role="switch"
            aria-checked={optIn}
            aria-label="Toggle Maloca Data Node Consensus Participation"
            title="Toggle Maloca Data Node Consensus Participation"
            onClick={handleToggleOptIn}
            className={`relative w-12 h-6 rounded-full transition-all duration-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400/50 ${
              optIn
                ? "bg-emerald-500 shadow-[0_0_12px_rgba(16,185,129,0.4)]"
                : "bg-white/20"
            }`}
          >
            <span
              className={`absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform duration-300 shadow-md ${
                optIn ? "translate-x-6" : "translate-x-0"
              }`}
            />
          </button>
        </div>
      </div>

      {/* Storage & Quota Allocation Section */}
      <div className="py-6 border-b border-white/10 space-y-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 text-sm font-semibold text-white/90">
            <HardDrive className="w-4 h-4 text-emerald-400" aria-hidden="true" />
            <span>Database Storage Allocation</span>
          </div>
          <span className="text-xs font-mono text-white/70">
            {formattedDbSize} GB used / {formattedQuota} GB quota ({usagePercentage}%)
          </span>
        </div>

        {/* Storage Bar */}
        <div className="w-full bg-white/10 rounded-full h-3 overflow-hidden p-0.5 border border-white/5">
          <div
            className={`h-full rounded-full transition-all duration-500 ${
              usagePercentage > 90
                ? "bg-red-500"
                : usagePercentage > 75
                ? "bg-amber-400"
                : "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.5)]"
            }`}
            style={{ width: `${usagePercentage}%` }}
          />
        </div>

        {/* Storage Quota Slider & Input */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pt-2">
          <div className="flex items-center gap-3 flex-1">
            <label htmlFor="datanode-quota-slider" className="text-xs text-white/60 whitespace-nowrap">
              Adjust Quota (MB):
            </label>
            <input
              id="datanode-quota-slider"
              type="range"
              min={1000}
              max={20000}
              step={500}
              value={quotaMb}
              onChange={(e) => handleQuotaChange(Number.parseInt(e.target.value, 10))}
              disabled={!optIn}
              aria-label="Storage quota range slider in MB"
              className="w-full h-1.5 bg-white/20 rounded-lg appearance-none cursor-pointer accent-emerald-400 disabled:opacity-40 disabled:cursor-not-allowed"
            />
          </div>
          <div className="flex items-center gap-2">
            <input
              id="datanode-quota-input"
              type="number"
              min={500}
              max={50000}
              value={quotaMb}
              onChange={(e) => handleQuotaChange(Number.parseInt(e.target.value, 10) || 0)}
              disabled={!optIn}
              aria-label="Storage quota input in MB"
              className="w-24 bg-black/50 border border-white/15 focus:border-emerald-400/60 rounded-lg px-3 py-1 text-xs font-mono text-white outline-none disabled:opacity-40 disabled:cursor-not-allowed"
            />
            <span className="text-xs text-white/50 font-mono">MB</span>
          </div>
        </div>
      </div>

      {/* Network Metrics Cards */}
      <div className="py-6 border-b border-white/10">
        <h3 className="text-xs uppercase tracking-wider text-white/50 font-semibold mb-4 flex items-center gap-2">
          <Activity className="w-3.5 h-3.5 text-emerald-400" aria-hidden="true" />
          Network & Synchronization Stats
        </h3>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          <div className="bg-white/5 border border-white/5 rounded-xl p-3.5">
            <div className="flex items-center gap-2 text-white/50 text-xs mb-1">
              <Users className="w-3.5 h-3.5 text-blue-400" aria-hidden="true" />
              Connected Peers
            </div>
            <div className="text-lg font-bold font-mono text-white">
              {optIn ? metrics.connectedPeers : 0}
            </div>
          </div>

          <div className="bg-white/5 border border-white/5 rounded-xl p-3.5">
            <div className="flex items-center gap-2 text-white/50 text-xs mb-1">
              <Layers className="w-3.5 h-3.5 text-emerald-400" aria-hidden="true" />
              Synced Records
            </div>
            <div className="text-lg font-bold font-mono text-white">
              {metrics.totalSyncedRecords.toLocaleString()}
            </div>
          </div>

          <div className="bg-white/5 border border-white/5 rounded-xl p-3.5">
            <div className="flex items-center gap-2 text-white/50 text-xs mb-1">
              <Radio className="w-3.5 h-3.5 text-purple-400" aria-hidden="true" />
              Bandwidth
            </div>
            <div className="text-lg font-bold font-mono text-white">
              {optIn ? `${metrics.bandwidthUsageMbps} Mbps` : "0 Mbps"}
            </div>
          </div>

          <div className="bg-white/5 border border-white/5 rounded-xl p-3.5">
            <div className="flex items-center gap-2 text-white/50 text-xs mb-1">
              <Wifi className="w-3.5 h-3.5 text-amber-400" aria-hidden="true" />
              Peer Latency
            </div>
            <div className="text-lg font-bold font-mono text-white">
              {optIn ? `${metrics.latencyMs} ms` : "N/A"}
            </div>
          </div>
        </div>
      </div>

      {/* Action Footer */}
      <div className="pt-6 flex flex-col sm:flex-row items-center justify-between gap-4">
        <div className="flex items-center gap-2 text-xs text-white/40">
          <ShieldCheck className="w-4 h-4 text-emerald-400/80" aria-hidden="true" />
          <span>Privacy P4 compliant: Data deltas are end-to-end encrypted.</span>
        </div>

        <div className="flex items-center gap-3 w-full sm:w-auto">
          <button
            type="button"
            onClick={handleRefreshStatus}
            disabled={isRefreshing}
            aria-label="Refresh Data Node Status"
            title="Refresh Data Node Status"
            className="flex-1 sm:flex-none px-3.5 py-2 rounded-xl bg-white/5 hover:bg-white/10 border border-white/10 text-xs font-medium text-white/80 transition-colors flex items-center justify-center gap-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400/50 disabled:opacity-50"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? "animate-spin" : ""}`} aria-hidden="true" />
            Refresh
          </button>

          <button
            type="button"
            onClick={handleSyncTrigger}
            disabled={!optIn || isSyncing}
            aria-label="Trigger Manual Consensus Data Sync"
            title="Trigger Manual Consensus Data Sync"
            className="flex-1 sm:flex-none px-4 py-2 rounded-xl bg-emerald-500 hover:bg-emerald-400 text-black font-semibold text-xs transition-colors flex items-center justify-center gap-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400 disabled:opacity-40 disabled:cursor-not-allowed shadow-[0_0_12px_rgba(16,185,129,0.3)]"
          >
            {isSyncing ? (
              <>
                <RefreshCw className="w-3.5 h-3.5 animate-spin" aria-hidden="true" />
                Syncing...
              </>
            ) : (
              <>
                <Zap className="w-3.5 h-3.5 fill-current" aria-hidden="true" />
                Trigger Sync
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

export default DataNodeDashboard;
