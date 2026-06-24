import { Bell, Shield, Volume2 } from "lucide-react";
import { useEffect, useState } from "react";
import { getApiUrl } from "../../api/client";

interface NotificationSettings {
  enabled_islands: string[];
  sound_enabled: boolean;
}

export default function NotificationsSettings({ token }: { token: string }) {
  const [settings, setSettings] = useState<NotificationSettings>({
    enabled_islands: ["system", "memory", "agents", "errors"],
    sound_enabled: true,
  });
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch(getApiUrl("/v1/settings/notifications"), {
      headers: { "X-Xavier-Token": token },
    })
      .then((res) => res.json())
      .then((data) => {
        setSettings(data);
        setLoading(false);
      })
      .catch((err) => {
        console.error("Failed to load notification settings:", err);
        setLoading(false);
      });
  }, [token]);

  const updateSettings = async (newSettings: Partial<NotificationSettings>) => {
    const updated = { ...settings, ...newSettings };
    setSettings(updated);

    try {
      await fetch(getApiUrl("/v1/settings/notifications"), {
        method: "PATCH",
        headers: {
          "Content-Type": "application/json",
          "X-Xavier-Token": token,
        },
        body: JSON.stringify(newSettings),
      });
    } catch (err) {
      console.error("Failed to update notification settings:", err);
    }
  };

  const toggleIsland = (island: string) => {
    const enabled = settings.enabled_islands.includes(island);
    const newIslands = enabled
      ? settings.enabled_islands.filter((i) => i !== island)
      : [...settings.enabled_islands, island];
    updateSettings({ enabled_islands: newIslands });
  };

  if (loading) {
    return <div className="p-8 text-white/50 font-mono text-sm">Loading...</div>;
  }

  return (
    <div className="p-8 max-w-2xl flex flex-col gap-8">
      <div>
        <h2 className="text-3xl font-light text-white tracking-tight">
          Notification Preferences
        </h2>
        <p className="text-sm text-white/40 mt-1">
          Configure real-time alerts and audio feedback.
        </p>
      </div>

      <div className="space-y-6">
        <div className="flex flex-col gap-4">
          <h3 className="text-[10px] uppercase text-white/50 tracking-widest flex items-center gap-2">
            <Bell className="w-3 h-3" />
            Category Islands
          </h3>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {["system", "memory", "agents", "errors"].map((island) => (
              <button
                key={island}
                onClick={() => toggleIsland(island)}
                className={`flex items-center justify-between p-4 rounded-xl border transition-all ${
                  settings.enabled_islands.includes(island)
                    ? "bg-[#39ff14]/5 border-[#39ff14]/30 text-white"
                    : "bg-white/[0.02] border-white/5 text-white/40 hover:border-white/10"
                }`}
              >
                <span className="text-sm capitalize">{island}</span>
                <div
                  className={`w-2 h-2 rounded-full ${
                    settings.enabled_islands.includes(island)
                      ? "bg-[#39ff14] shadow-[0_0_8px_#39ff14]"
                      : "bg-white/10"
                  }`}
                />
              </button>
            ))}
          </div>
        </div>

        <div className="pt-4 border-t border-white/5">
          <h3 className="text-[10px] uppercase text-white/50 tracking-widest mb-4 flex items-center gap-2">
            <Volume2 className="w-3 h-3" />
            Audio Feedback
          </h3>
          <ToggleRow
            label="Notification Sound"
            description="Play a subtle beep for errors and warnings."
            checked={settings.sound_enabled}
            onChange={(val) => updateSettings({ sound_enabled: val })}
          />
        </div>

        <div className="pt-4 border-t border-white/5">
          <h3 className="text-[10px] uppercase text-white/50 tracking-widest mb-4 flex items-center gap-2">
            <Shield className="w-3 h-3" />
            System Rules
          </h3>
          <div className="p-4 rounded-xl bg-indigo-500/5 border border-indigo-500/20 text-[11px] text-indigo-300/80 leading-relaxed">
            Note: Critical system failures bypass category filters to ensure stability awareness.
          </div>
        </div>
      </div>
    </div>
  );
}

function ToggleRow({
  label,
  description,
  checked,
  onChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  onChange: (val: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between p-4 rounded-xl bg-white/[0.02] border border-white/5 hover:border-white/10 transition-colors">
      <div>
        <h4 className="text-white/90 text-sm font-medium mb-1">{label}</h4>
        <p className="text-xs text-white/40">{description}</p>
      </div>
      <button
        onClick={() => onChange(!checked)}
        className={`relative w-10 h-6 rounded-full transition-all duration-300 ${
          checked ? "bg-[#39ff14]" : "bg-white/10"
        }`}
      >
        <div
          className={`absolute top-1 left-1 w-4 h-4 rounded-full bg-white transition-transform duration-300 ${
            checked ? "translate-x-4" : "translate-x-0"
          }`}
        />
      </button>
    </div>
  );
}
