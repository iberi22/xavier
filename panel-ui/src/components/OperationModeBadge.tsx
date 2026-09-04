import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { getApiUrl } from "../api/client";
import { useAuthStore } from "../auth/AuthProvider";

export default function OperationModeBadge() {
  const token = useAuthStore((state) => state.token);
  const [mode, setMode] = useState<"local" | "cloud" | "degraded" | "offline">(
    "cloud",
  );
  const [activeProvider, setActiveProvider] = useState<string>("unknown");
  const [modelName, setModelName] = useState<string>("");

  useEffect(() => {
    let isMounted = true;

    const fetchStatus = async () => {
      try {
        const headers: Record<string, string> = {
          "Content-Type": "application/json",
        };
        if (token) {
          headers["X-Xavier-Token"] = token;
          headers["Authorization"] = `Bearer ${token}`;
        }

        // Fetch from /health first as it has most details
        const healthRes = await fetch(getApiUrl("/health"), { headers }).catch(
          () => null,
        );
        let healthData: any = null;
        if (healthRes && healthRes.ok) {
          healthData = await healthRes.json().catch(() => null);
        }

        // Also try `/provider/status` or `/v1/providers/status` if healthData doesn't have mode
        let providerData: any = null;
        if (!healthData || !healthData.mode) {
          const providerRes = await fetch(getApiUrl("/provider/status"), {
            headers,
          }).catch(() => null);
          if (providerRes && providerRes.ok) {
            providerData = await providerRes.json().catch(() => null);
          } else {
            const v1Res = await fetch(getApiUrl("/v1/providers/status"), {
              headers,
            }).catch(() => null);
            if (v1Res && v1Res.ok) {
              providerData = await v1Res.json().catch(() => null);
            }
          }
        }

        if (!isMounted) return;

        // If both failed to return anything or network is down
        if (!healthData && !providerData) {
          setMode("offline");
          return;
        }

        // Extract mode
        const rawMode = healthData?.mode || providerData?.mode || "cloud";
        const active =
          healthData?.llm?.provider || providerData?.active || "unknown";
        const model = healthData?.llm?.model || "";

        // Normalize mode
        let normalizedMode: "local" | "cloud" | "degraded" = "cloud";
        if (rawMode === "local" || rawMode === "local-healthy") {
          normalizedMode = "local";
        } else if (rawMode === "degraded" || rawMode === "local-degraded") {
          normalizedMode = "degraded";
        } else if (rawMode === "cloud" || rawMode === "cloud-fallback") {
          normalizedMode = "cloud";
        } else {
          normalizedMode = "cloud"; // Default fallback as per instruction
        }

        // Check reachability overrides
        const reachable =
          healthData?.llm?.reachable !== false &&
          providerData?.local_reachable !== false;
        if (!reachable && normalizedMode === "local") {
          normalizedMode = "degraded";
        }

        setMode(normalizedMode);
        setActiveProvider(active);
        setModelName(model);
      } catch (error) {
        console.error("Error checking operation status:", error);
        if (isMounted) {
          setMode("offline");
        }
      }
    };

    void fetchStatus();
    const interval = setInterval(fetchStatus, 10000); // Poll every ~10s

    return () => {
      isMounted = false;
      clearInterval(interval);
    };
  }, [token]);

  // Render configuration mapping based on mode
  let dotColor = "bg-neutral-500";
  let borderStyle = "border-neutral-500/20";
  let glowShadow = "";
  let emoji = "🔘";
  let title = "Offline";
  let tooltip = "Sin estado del servidor. No se pudo conectar con Xavier.";
  let textColorStyle = { color: "#a3a3a3" };

  if (mode === "local") {
    dotColor = "bg-[#39ff14]";
    borderStyle = "border-[#39ff14]/30";
    glowShadow = "shadow-[0_0_8px_rgba(57,255,20,0.4)]";
    emoji = "🦙";
    title = "Local";
    tooltip = "Operando 100% local. Tus datos no salen de esta máquina.";
    textColorStyle = { color: "#39ff14" };
  } else if (mode === "cloud") {
    dotColor = "bg-blue-400";
    borderStyle = "border-blue-400/30";
    glowShadow = "shadow-[0_0_8px_rgba(96,165,250,0.4)]";
    emoji = "☁️";
    title = "Cloud";
    tooltip =
      "Operando en modo Cloud. Se pueden aplicar cargos por el uso de APIs.";
    textColorStyle = { color: "#60a5fa" };
  } else if (mode === "degraded") {
    dotColor = "bg-amber-400";
    borderStyle = "border-amber-400/30";
    glowShadow = "shadow-[0_0_8px_rgba(251,191,36,0.4)]";
    emoji = "⚠️";
    title = "Degradado";
    tooltip =
      "El LLM local no responde. Las solicitudes están fallando o se están redireccionando.";
    textColorStyle = { color: "#fbbf24" };
  }

  const capitalize = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

  return (
    <motion.div
      layout
      className={`relative group flex items-center bg-[#0a0a0a]/80 backdrop-blur-md border ${borderStyle} rounded-full px-3 py-1 h-7 text-white/80 shrink-0 text-[10px] font-mono select-none cursor-default`}
    >
      {/* Status Dot */}
      <div className="relative flex h-1.5 w-1.5 mr-2">
        {mode !== "offline" && (
          <span
            className={`animate-ping absolute inline-flex h-full w-full rounded-full opacity-75 ${dotColor}`}
          />
        )}
        <span
          className={`relative inline-flex rounded-full h-1.5 w-1.5 ${dotColor} ${glowShadow}`}
        />
      </div>

      {/* Emoji and Title */}
      <span className="font-semibold" style={textColorStyle}>
        {emoji} {title}
      </span>

      {mode !== "offline" && (
        <>
          <span className="mx-1.5 text-white/20">|</span>
          <span className="text-white/60">
            {capitalize(activeProvider)}
            {mode === "local" && modelName && ` (${modelName})`}
          </span>
        </>
      )}

      {/* Custom HTML/CSS Tooltip */}
      <div className="absolute top-full mt-2 left-1/2 -translate-x-1/2 hidden group-hover:flex flex-col bg-[#0a0a0a]/95 border border-white/10 p-3 rounded-xl shadow-2xl text-[10px] text-white/80 whitespace-nowrap min-w-[220px] gap-1.5 z-[100] backdrop-blur-md transition-all duration-200">
        <div className="font-bold border-b border-white/10 pb-1.5 mb-1 flex items-center justify-between">
          <span className="tracking-wider uppercase">Estado del Sistema</span>
          <span style={textColorStyle}>{title}</span>
        </div>
        <div>
          <span className="text-white/40 font-semibold mr-1">Modo:</span>
          <span className="capitalize">{mode}</span>
        </div>
        {mode !== "offline" && (
          <>
            <div>
              <span className="text-white/40 font-semibold mr-1">
                Proveedor:
              </span>
              <span className="capitalize">{activeProvider}</span>
            </div>
            {modelName && (
              <div>
                <span className="text-white/40 font-semibold mr-1">
                  Modelo:
                </span>
                <span>{modelName}</span>
              </div>
            )}
          </>
        )}
        <div className="text-white/70 italic mt-1.5 pt-1.5 border-t border-white/5 whitespace-normal max-w-[240px] leading-normal">
          {tooltip}
        </div>
      </div>
    </motion.div>
  );
}
