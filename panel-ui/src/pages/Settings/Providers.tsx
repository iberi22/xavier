import { Cpu, Globe, RefreshCcw, Save, ShieldCheck, Zap } from "lucide-react";
import { motion } from "motion/react";
import React, { useCallback, useEffect, useState } from "react";
import {
  ApiClient,
  type ProviderConfig,
  type ProviderQuota,
  type SystemScan,
} from "../../api/client";
import { ApiKeyInput } from "../../components/ApiKeyInput";
import { CliAgentList } from "../../components/CliAgentList";
import { HeadlessToggle } from "../../components/HeadlessToggle";
import { OllamaModelManager } from "../../components/OllamaModelManager";
import { ProviderSelector } from "../../components/ProviderSelector";
import { QuotaTable } from "../../components/QuotaTable";

interface ProvidersPageProps {
  token: string;
}

export default function ProvidersPage({ token }: ProvidersPageProps) {
  const [client] = useState(() => new ApiClient(token));
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const [systemScan, setSystemScan] = useState<SystemScan | null>(null);
  const [quotas, setQuotas] = useState<ProviderQuota[]>([]);
  const [configs, setConfigs] = useState<ProviderConfig[]>([]);

  const [activeProvider, setActiveProvider] = useState("gemini");
  const [headlessEnabled, setHeadlessEnabled] = useState(true);
  const [headlessPort, setHeadlessPort] = useState(8006);

  const fetchData = useCallback(async () => {
    try {
      const [scan, quotaList, configList] = await Promise.all([
        client.systemScan(),
        client.getProvidersQuota(),
        client.getProvidersConfig(),
      ]);
      setSystemScan(scan);
      setQuotas(quotaList);
      setConfigs(configList.providers);

      // Assume first configured cloud provider is active or use local
      const firstConfigured = scan.providers.find((p) => p.configured);
      if (firstConfigured) setActiveProvider(firstConfigured.name);
    } catch (e) {
      console.error("Failed to fetch provider data:", e);
    } finally {
      setLoading(false);
    }
  }, [client]);

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 30000);
    return () => clearInterval(interval);
  }, [fetchData]);

  const handleSave = async () => {
    setSaving(true);
    try {
      await client.updateProvidersConfig(configs);
      await fetchData();
    } catch (e) {
      alert("Failed to save configuration");
    } finally {
      setSaving(false);
    }
  };

  const updateConfig = (name: string, fields: Partial<ProviderConfig>) => {
    setConfigs((prev) =>
      prev.map((c) => (c.provider === name ? { ...c, ...fields } : c)),
    );
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <RefreshCcw className="w-8 h-8 text-[#39ff14] animate-spin opacity-50" />
      </div>
    );
  }

  const mappedQuotas = quotas.map((q) => ({
    provider: q.provider,
    tier: q.weekly_quota > 500000 ? "Pro" : "Free",
    requests: `${q.used_today / 100}K / 5K`, // Simulated request count
    tokens: `${(q.used_today / 1000).toFixed(1)}K / ${(q.weekly_quota / 1000).toFixed(0)}K`,
    reset: "2h",
    status: q.rate_limited_until
      ? "red"
      : ((q.used_weekly / q.weekly_quota > 0.8 ? "yellow" : "green") as any),
  }));

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="flex flex-col gap-8 pb-12"
    >
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-3xl font-light text-white tracking-tight">
            AI Provider Ecosystem
          </h2>
          <p className="text-sm text-white/40 mt-1">
            Manage LLM backends, API keys and system-wide routing.
          </p>
        </div>
        <button
          onClick={handleSave}
          disabled={saving}
          className="flex items-center gap-2 px-6 py-2.5 bg-[#39ff14] text-black font-bold rounded-xl hover:shadow-[0_0_20px_rgba(57,255,20,0.4)] transition-all disabled:opacity-50"
        >
          {saving ? (
            <RefreshCcw className="w-4 h-4 animate-spin" />
          ) : (
            <Save className="w-4 h-4" />
          )}
          {saving ? "Saving..." : "Apply Changes"}
        </button>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
        <div className="lg:col-span-2 space-y-8">
          {/* Active Provider & General Status */}
          <section className="bg-[#050505]/30 border border-white/5 rounded-[32px] p-8">
            <div className="flex items-center gap-2 mb-6">
              <Globe className="w-4 h-4 text-[#39ff14]" />
              <h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/60">
                Primary Routing
              </h3>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-8 items-end">
              <ProviderSelector
                providers={
                  systemScan?.providers.map((p) => ({
                    name: p.name,
                    status: p.configured ? "running" : "error",
                    configured: p.configured,
                  })) || []
                }
                activeProvider={activeProvider}
                onSwitch={setActiveProvider}
              />
              <div className="flex gap-4 p-4 bg-white/5 rounded-2xl border border-white/5">
                <div className="p-2 bg-[#39ff14]/10 rounded-lg">
                  <ShieldCheck className="w-5 h-5 text-[#39ff14]" />
                </div>
                <div>
                  <p className="text-[10px] text-white/40 uppercase font-bold tracking-wider">
                    Security Policy
                  </p>
                  <p className="text-sm text-white/90">Hardware-Backed Keys</p>
                </div>
              </div>
            </div>
          </section>

          {/* Ollama Model Manager */}
          <OllamaModelManager token={token} onModelChanged={fetchData} />

          {/* API Credentials */}
          <section className="space-y-6">
            <div className="flex items-center gap-2 px-2">
              <Cpu className="w-4 h-4 text-white/40" />
              <h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/40">
                Model Credentials
              </h3>
            </div>
            <div className="grid gap-4">
              {configs.map((config) => (
                <div
                  key={config.provider}
                  className="bg-[#050505]/30 border border-white/5 rounded-2xl p-6"
                >
                  <div className="flex items-center justify-between mb-4">
                    <h4 className="text-sm font-bold capitalize">
                      {config.provider}
                    </h4>
                    <div className="flex items-center gap-4">
                      <div className="flex flex-col items-end">
                        <span className="text-[9px] text-white/30 uppercase font-bold">
                          Model
                        </span>
                        <input
                          value={config.model}
                          onChange={(e) =>
                            updateConfig(config.provider, {
                              model: e.target.value,
                            })
                          }
                          className="bg-transparent text-right text-xs text-white/80 focus:text-[#39ff14] outline-none"
                        />
                      </div>
                    </div>
                  </div>
                  <ApiKeyInput
                    label="API Key"
                    value={config.api_key || ""}
                    onChange={(val) =>
                      updateConfig(config.provider, { api_key: val })
                    }
                    onTest={() =>
                      client.testProvider(config.provider).then(() => {})
                    }
                    onRemove={() =>
                      updateConfig(config.provider, { api_key: "" })
                    }
                  />
                  {config.provider === "local" && (
                    <div className="mt-4">
                      <label className="text-[10px] uppercase text-white/50 tracking-widest block mb-2">
                        Endpoint URL
                      </label>
                      <input
                        value={config.base_url || ""}
                        onChange={(e) =>
                          updateConfig(config.provider, {
                            base_url: e.target.value,
                          })
                        }
                        className="w-full bg-black/40 border border-white/5 rounded-lg px-4 py-2 text-xs font-mono outline-none focus:border-[#39ff14]/30"
                        placeholder="http://localhost:11434"
                      />
                    </div>
                  )}
                </div>
              ))}
            </div>
          </section>
        </div>

        <div className="space-y-8">
          {/* Headless Toggle */}
          <HeadlessToggle
            enabled={headlessEnabled}
            port={headlessPort}
            onToggle={setHeadlessEnabled}
            onPortChange={setHeadlessPort}
          />

          {/* CLI Agents */}
          <section className="space-y-4">
            <div className="flex items-center gap-2 px-2">
              <Zap className="w-4 h-4 text-white/40" />
              <h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/40">
                CLI Agents
              </h3>
            </div>
            <CliAgentList
              agents={[
                { name: "Xavier Core", status: "logged_in", enabled: true },
                { name: "Code Graph", status: "not_logged_in", enabled: true },
                {
                  name: "Swarm Master",
                  status: "not_installed",
                  enabled: false,
                },
              ]}
              onToggle={() => {}}
              onLogin={() => {}}
            />
          </section>
        </div>
      </div>

      {/* Quota Table */}
      <section className="space-y-4 mt-4">
        <div className="flex items-center justify-between px-2">
          <div className="flex items-center gap-2">
            <RefreshCcw className="w-4 h-4 text-white/40" />
            <h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/40">
              Usage & Quotas
            </h3>
          </div>
          <span className="text-[10px] text-white/30 italic">
            Auto-refreshes every 30s
          </span>
        </div>
        <QuotaTable quotas={mappedQuotas} />
      </section>
    </motion.div>
  );
}
