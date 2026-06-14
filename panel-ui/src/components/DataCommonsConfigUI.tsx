import React, { useEffect, useState } from "react";
import {
  Database,
  ShieldAlert,
  Sparkles,
  Check,
  TrendingUp,
  Coins,
  ArrowUpRight,
  ArrowDownLeft,
} from "lucide-react";
import { ApiClient, DataCommonsConfig } from "../api/client";

export default function DataCommonsConfigUI({ token }: { token: string }) {
  const [config, setConfig] = useState<DataCommonsConfig>({
    enabled: false,
    consent_given: false,
    wallet_address: "",
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const fetchConfig = async () => {
      try {
        const client = new ApiClient(token);
        const res = await client.getDataCommons();
        setConfig(res.data);
      } catch (err: any) {
        setError(err.message || "Failed to load Data Commons config");
      } finally {
        setLoading(false);
      }
    };
    fetchConfig();
  }, [token]);

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    try {
      const client = new ApiClient(token);
      await client.optInDataCommons(config);
      setSaved(true);
      setTimeout(() => setSaved(false), 3000);
    } catch (err: any) {
      setError(err.message || "Failed to save configuration");
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return <div className="text-white/40 text-sm">Loading Data Commons...</div>;
  }

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <StatsCard
          label="Estimated $XAV"
          value="452.80"
          subValue="~$12.40 USD"
          icon={<Coins className="text-emerald-400" size={16} />}
        />
        <StatsCard
          label="Data Quality Score"
          value="0.98"
          subValue="Top 5% of nodes"
          icon={<TrendingUp className="text-blue-400" size={16} />}
        />
        <StatsCard
          label="Contribution Level"
          value="Lvl 4"
          subValue="1.2k embeddings"
          icon={<Sparkles className="text-orange-400" size={16} />}
        />
      </div>

      <div className="bg-[#050505]/50 border border-emerald-900/30 rounded-2xl overflow-hidden relative">
        <div className="absolute top-0 left-0 w-full h-1 bg-gradient-to-r from-emerald-500/0 via-emerald-500 to-emerald-500/0 opacity-20" />

        <div className="p-6">
        <div className="flex items-start justify-between mb-6">
          <div className="flex items-start gap-4">
            <div className="p-3 bg-emerald-500/10 rounded-xl">
              <Database className="w-6 h-6 text-emerald-400" />
            </div>
            <div>
              <h3 className="text-xl font-light text-white tracking-tight flex items-center gap-2">
                Xavier Data Commons
                <span className="px-2 py-0.5 rounded-full bg-emerald-500/20 text-emerald-400 text-[10px] uppercase tracking-widest font-bold">
                  Rewards
                </span>
              </h3>
              <p className="text-sm text-white/50 mt-1 max-w-md">
                Opt-in to share anonymized usage metrics, embeddings, and interactions to build better open-source models. Earn $XAV tokens for contributing to the decentralized AI network.
              </p>
            </div>
          </div>
        </div>

        {error && (
          <div className="mb-6 p-4 rounded-xl bg-red-500/10 border border-red-500/20 flex items-center gap-3">
            <ShieldAlert className="w-5 h-5 text-red-400" />
            <span className="text-sm text-red-200">{error}</span>
          </div>
        )}

        <div className="space-y-6">
          <div className="flex items-center justify-between p-4 rounded-xl bg-white/5 border border-white/5">
            <div>
              <h4 className="text-sm font-medium text-white/90">Enable Data Telemetry</h4>
              <p className="text-xs text-white/40 mt-1">Allow local embeddings to be sent anonymously.</p>
            </div>
            <button
              onClick={() => setConfig({ ...config, enabled: !config.enabled })}
              className={`relative w-12 h-7 rounded-full transition-all duration-300 ${
                config.enabled ? "bg-emerald-500 shadow-[0_0_15px_rgba(16,185,129,0.4)]" : "bg-white/10"
              }`}
            >
              <div
                className={`absolute top-1 left-1 w-5 h-5 rounded-full bg-white transition-transform duration-300 ${
                  config.enabled ? "translate-x-5" : "translate-x-0 opacity-60"
                }`}
              />
            </button>
          </div>

          <div className="flex items-center justify-between p-4 rounded-xl bg-white/5 border border-white/5">
            <div>
              <h4 className="text-sm font-medium text-white/90">GDPR / Legal Consent</h4>
              <p className="text-xs text-white/40 mt-1">I agree to the anonymous data collection policy.</p>
            </div>
            <button
              onClick={() => setConfig({ ...config, consent_given: !config.consent_given })}
              className={`relative w-12 h-7 rounded-full transition-all duration-300 ${
                config.consent_given ? "bg-emerald-500 shadow-[0_0_15px_rgba(16,185,129,0.4)]" : "bg-white/10"
              }`}
            >
              <div
                className={`absolute top-1 left-1 w-5 h-5 rounded-full bg-white transition-transform duration-300 ${
                  config.consent_given ? "translate-x-5" : "translate-x-0 opacity-60"
                }`}
              />
            </button>
          </div>

          <div className="p-4 rounded-xl bg-white/5 border border-white/5">
            <h4 className="text-sm font-medium text-white/90 mb-1">Solana Wallet Address (Optional)</h4>
            <p className="text-xs text-white/40 mb-3">Link your wallet to receive airdrops for your data contribution.</p>
            <input
              type="text"
              value={config.wallet_address || ""}
              onChange={(e) => setConfig({ ...config, wallet_address: e.target.value })}
              placeholder="e.g. HN7cABqLq46Es1jh92dQQisAq662SmxELLLsHHe4YWrH"
              className="w-full bg-black/50 border border-white/10 rounded-lg px-4 py-2 text-sm text-white/90 outline-none focus:border-emerald-500 transition-colors"
            />
          </div>
        </div>

          <div className="mt-8 flex items-center justify-between border-t border-white/5 pt-6">
            <div className="flex items-center gap-2 text-xs text-white/30">
              <ShieldAlert className="w-4 h-4" />
              Your data remains encrypted in transit.
            </div>
            <button
              onClick={handleSave}
              disabled={saving || (!config.consent_given && config.enabled)}
              className="px-6 py-2 bg-white text-black hover:bg-emerald-400 font-medium rounded-lg transition-colors flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {saving ? (
                <Sparkles className="w-4 h-4 animate-spin" />
              ) : saved ? (
                <Check className="w-4 h-4" />
              ) : (
                <Sparkles className="w-4 h-4" />
              )}
              {saved ? "Saved" : "Save Preferences"}
            </button>
          </div>
        </div>
      </div>

      {/* Transaction History (Mock) */}
      <section className="space-y-4">
        <h4 className="text-xs font-semibold text-white/40 uppercase tracking-widest px-2">
          Reward History
        </h4>
        <div className="bg-black/30 border border-white/5 rounded-2xl divide-y divide-white/5">
          <TransactionRow
            date="2026-06-12"
            type="Embedding Contribution"
            amount="+12.5 XAV"
          />
          <TransactionRow
            date="2026-06-11"
            type="Uptime Reward"
            amount="+5.0 XAV"
          />
          <TransactionRow
            date="2026-06-10"
            type="Network Validation"
            amount="+2.2 XAV"
          />
        </div>
      </section>
    </div>
  );
}

function StatsCard({
  label,
  value,
  subValue,
  icon,
}: {
  label: string;
  value: string;
  subValue: string;
  icon: React.ReactNode;
}) {
  return (
    <div className="bg-black/40 border border-white/5 rounded-2xl p-4 flex flex-col gap-1">
      <div className="flex items-center justify-between">
        <span className="text-[10px] uppercase tracking-widest text-white/30 font-bold">
          {label}
        </span>
        {icon}
      </div>
      <div className="text-xl font-light text-white mt-1">{value}</div>
      <div className="text-[10px] text-white/40">{subValue}</div>
    </div>
  );
}

function TransactionRow({
  date,
  type,
  amount,
}: {
  date: string;
  type: string;
  amount: string;
}) {
  return (
    <div className="flex items-center justify-between p-4 hover:bg-white/[0.02] transition-all">
      <div className="flex items-center gap-3">
        <div className="p-1.5 rounded-lg bg-emerald-500/10">
          <ArrowUpRight className="text-emerald-400" size={14} />
        </div>
        <div className="flex flex-col">
          <span className="text-xs font-medium text-white/80">{type}</span>
          <span className="text-[10px] text-white/30">{date}</span>
        </div>
      </div>
      <span className="text-xs font-mono text-emerald-400 font-bold">
        {amount}
      </span>
    </div>
  );
}
