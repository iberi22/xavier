import { Cloud } from "lucide-react";
import React, { useEffect, useState } from "react";
import { ApiClient, type CloudNodeConfig } from "../api/client";

export function CloudRelayConfig({ token }: { token: string }) {
  const [client] = useState(() => new ApiClient(token));
  const [cloudSettings, setCloudSettings] = useState<CloudNodeConfig>({
    url: "",
    token: "",
    instance_id: "",
  });
  const [cloudSaved, setCloudSaved] = useState(false);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    client.getCloudNode()
      .then((res) => {
        if (res.status === "ok" && res.data) {
          setCloudSettings({
            url: res.data.url || "",
            token: res.data.token || "",
            instance_id: res.data.instance_id || "",
          });
        }
      })
      .catch((err) => console.error("Failed to load cloud node config", err));
  }, [client]);

  const handleSaveCloud = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    try {
      const res = await client.updateCloudNode(cloudSettings);
      if (res.status === "ok") {
        setCloudSaved(true);
        setTimeout(() => setCloudSaved(false), 2000);
      }
    } catch (err) {
      console.error("Failed to save cloud settings", err);
    } finally {
      setLoading(false);
    }
  };

  return (
    <section className="bg-[#050505]/30 border border-white/5 rounded-2xl p-6">
      <div className="flex items-center gap-3 mb-5">
        <div className="p-2 rounded-lg bg-orange-500/10 text-orange-400">
          <Cloud size={18} />
        </div>
        <div>
          <h2 className="font-semibold text-white/90">Zero-Trust Cloud Relay</h2>
          <p className="text-xs text-white/40 uppercase tracking-widest mt-1">
            Configure Supabase / Neon endpoints
          </p>
        </div>
      </div>

      <form onSubmit={handleSaveCloud} className="space-y-4">
        <div className="space-y-4">
          <div className="flex flex-col gap-1">
            <label className="text-[10px] uppercase text-white/50 tracking-widest">
              Supabase URL
            </label>
            <input
              type="text"
              value={cloudSettings.url}
              onChange={(e) =>
                setCloudSettings({ ...cloudSettings, url: e.target.value })
              }
              placeholder="https://xyz.supabase.co"
              className="w-full bg-black/40 border border-white/5 rounded-lg px-4 py-2 text-xs font-mono outline-none focus:border-[#39ff14]/30 text-white/80"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-[10px] uppercase text-white/50 tracking-widest">
              Service Token (API Key)
            </label>
            <input
              type="password"
              value={cloudSettings.token}
              onChange={(e) =>
                setCloudSettings({ ...cloudSettings, token: e.target.value })
              }
              placeholder="Your service_role key or anon key"
              className="w-full bg-black/40 border border-white/5 rounded-lg px-4 py-2 text-xs font-mono outline-none focus:border-[#39ff14]/30 text-white/80"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-[10px] uppercase text-white/50 tracking-widest">
              Namespace / Node ID
            </label>
            <input
              type="text"
              value={cloudSettings.instance_id}
              onChange={(e) =>
                setCloudSettings({
                  ...cloudSettings,
                  instance_id: e.target.value,
                })
              }
              placeholder="pgheart-namespace-id"
              className="w-full bg-black/40 border border-white/5 rounded-lg px-4 py-2 text-xs font-mono outline-none focus:border-[#39ff14]/30 text-white/80"
            />
          </div>
        </div>
        <div className="flex items-center gap-3 pt-2">
          <button
            type="submit"
            disabled={loading}
            className="px-4 py-2 bg-orange-500/20 text-orange-400 border border-orange-500/30 rounded-lg text-xs font-bold uppercase tracking-widest hover:bg-orange-500/30 transition-colors disabled:opacity-50"
          >
            {loading ? "Saving..." : "Save Relay Config"}
          </button>
          {cloudSaved && (
            <span className="text-xs text-[#39ff14]">✓ Settings Applied</span>
          )}
        </div>
      </form>
    </section>
  );
}
