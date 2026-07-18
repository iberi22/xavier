import { AlertCircle, Check, Download, Loader2, RefreshCw } from "lucide-react";
import React, { useCallback, useEffect, useState } from "react";
import { ApiClient } from "../api/client";

interface OllamaModelManagerProps {
  token: string;
  onModelChanged?: () => void;
}

export function OllamaModelManager({ token, onModelChanged }: OllamaModelManagerProps) {
  const [client] = useState(() => new ApiClient(token));
  const [models, setModels] = useState<string[]>([]);
  const [activeLLM, setActiveLLM] = useState<string>("");
  const [activeEmbedding, setActiveEmbedding] = useState<string>("");

  const [selectedModel, setSelectedModel] = useState<string>("");
  const [activeKind, setActiveKind] = useState<"llm" | "embedding">("llm");
  const [pullModelName, setPullModelName] = useState<string>("");

  const [loading, setLoading] = useState<boolean>(true);
  const [pulling, setPulling] = useState<boolean>(false);
  const [settingActive, setSettingActive] = useState<boolean>(false);

  const [error, setError] = useState<string | null>(null);
  const [pullError, setPullError] = useState<string | null>(null);
  const [pullSuccess, setPullSuccess] = useState<boolean>(false);
  const [activeSuccess, setActiveSuccess] = useState<boolean>(false);

  const loadData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [modelsResp, activeResp] = await Promise.all([
        client.getOllamaModels(),
        client.getOllamaActive(),
      ]);
      // Normalize Ollama /api/tags shape or plain string[]
      const raw = modelsResp.models || [];
      const names = raw
        .map((m) => (typeof m === "string" ? m : m?.name || ""))
        .filter((n): n is string => Boolean(n));
      setModels(names);
      setActiveLLM(activeResp.llm || activeResp.model || "");
      setActiveEmbedding(activeResp.embedding || "");

      const currentActive = activeResp.llm || activeResp.model || "";
      if (currentActive) {
        setSelectedModel(currentActive);
      } else if (names.length > 0) {
        setSelectedModel(names[0]);
      }
    } catch (e: any) {
      console.error("Failed to fetch Ollama model info:", e);
      setError("Ollama no responde en :11434");
    } finally {
      setLoading(false);
    }
  }, [client]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleSetActive = async () => {
    if (!selectedModel) return;
    setSettingActive(true);
    setError(null);
    setActiveSuccess(false);
    try {
      const resp = await client.setOllamaActive(selectedModel, activeKind);
      if (resp.ok || resp.success) {
        if (activeKind === "llm") {
          setActiveLLM(selectedModel);
        } else {
          setActiveEmbedding(selectedModel);
        }
        setActiveSuccess(true);
        setTimeout(() => setActiveSuccess(false), 3000);
        if (onModelChanged) {
          onModelChanged();
        }
      } else {
        setError(resp.error || "Failed to set active model");
      }
    } catch (e: any) {
      setError(e.message || "An error occurred while setting active model");
    } finally {
      setSettingActive(false);
    }
  };

  const handlePullModel = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!pullModelName.trim()) return;
    setPulling(true);
    setPullError(null);
    setPullSuccess(false);
    try {
      await client.pullOllamaModel(pullModelName.trim());
      setPullSuccess(true);
      setPullModelName("");
      await loadData();
      setTimeout(() => setPullSuccess(false), 3000);
    } catch (e: any) {
      setPullError(e.message || "An error occurred while pulling model");
    } finally {
      setPulling(false);
    }
  };

  return (
    <div className="bg-[#050505]/80 border border-white/10 rounded-[24px] p-6 backdrop-blur-xl shadow-2xl space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <div className="relative">
            <div className={`w-2.5 h-2.5 rounded-full ${error ? "bg-red-500" : "bg-[#39ff14]"}`} />
            {!error && (
              <div className="absolute inset-0 w-2.5 h-2.5 rounded-full bg-[#39ff14] animate-ping opacity-75" />
            )}
          </div>
          <h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/80">
            Ollama Model Manager
          </h3>
        </div>
        <button
          onClick={loadData}
          disabled={loading || pulling}
          aria-label="Refrescar modelos"
          className="p-1.5 hover:bg-white/5 rounded-lg transition-colors text-white/40 hover:text-white/80 disabled:opacity-30"
        >
          <RefreshCw className={`w-4 h-4 ${loading ? "animate-spin" : ""}`} />
        </button>
      </div>

      {error ? (
        <div className="flex items-start gap-3 bg-red-950/30 border border-red-500/20 rounded-xl p-4 text-red-400">
          <AlertCircle className="w-5 h-5 shrink-0 mt-0.5" />
          <div>
            <p className="text-sm font-semibold">{error}</p>
            <p className="text-xs text-red-400/60 mt-1">
              Asegúrate de que Ollama está corriendo (`ollama serve`) en localhost:11434.
            </p>
          </div>
        </div>
      ) : (
        <div className="space-y-6">
          {/* Active Status Header */}
          <div className="grid grid-cols-2 gap-4 bg-white/5 border border-white/5 p-4 rounded-xl text-xs">
            <div>
              <span className="text-white/40 block mb-0.5">Chat LLM Activo</span>
              <span className="font-mono text-white/90 truncate block">{activeLLM || "Ninguno"}</span>
            </div>
            <div>
              <span className="text-white/40 block mb-0.5">Embedding Activo</span>
              <span className="font-mono text-white/90 truncate block">{activeEmbedding || "Ninguno"}</span>
            </div>
          </div>

          {/* Selector & Switch Active */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <label htmlFor="model-select" className="text-[10px] uppercase text-white/50 tracking-widest block font-medium">
                Modelos Disponibles
              </label>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setActiveKind("llm")}
                  className={`text-[9px] px-2 py-0.5 rounded uppercase font-bold tracking-wider transition-all ${
                    activeKind === "llm"
                      ? "bg-[#39ff14]/10 text-[#39ff14] border border-[#39ff14]/20"
                      : "text-white/40 hover:text-white/70 border border-transparent"
                  }`}
                >
                  LLM
                </button>
                <button
                  type="button"
                  onClick={() => setActiveKind("embedding")}
                  className={`text-[9px] px-2 py-0.5 rounded uppercase font-bold tracking-wider transition-all ${
                    activeKind === "embedding"
                      ? "bg-[#39ff14]/10 text-[#39ff14] border border-[#39ff14]/20"
                      : "text-white/40 hover:text-white/70 border border-transparent"
                  }`}
                >
                  Embedding
                </button>
              </div>
            </div>

            <div className="flex gap-2">
              <select
                id="model-select"
                aria-label="Seleccionar modelo de Ollama"
                value={selectedModel}
                onChange={(e) => setSelectedModel(e.target.value)}
                disabled={models.length === 0 || settingActive}
                className="flex-1 bg-black/60 border border-white/10 rounded-xl px-4 py-2.5 text-xs font-mono text-white/90 outline-none focus:border-[#39ff14]/40"
              >
                {models.length === 0 ? (
                  <option value="">No hay modelos cargados</option>
                ) : (
                  models.map((model) => (
                    <option key={model} value={model}>
                      {model}
                    </option>
                  ))
                )}
              </select>

              <button
                type="button"
                onClick={handleSetActive}
                disabled={!selectedModel || settingActive || models.length === 0}
                className="px-4 py-2.5 bg-white/5 hover:bg-[#39ff14]/10 border border-white/10 hover:border-[#39ff14]/30 hover:text-[#39ff14] text-white/80 font-bold rounded-xl text-xs transition-all flex items-center justify-center gap-2 shrink-0 disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-white/80 disabled:hover:border-white/10"
              >
                {settingActive ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : activeSuccess ? (
                  <Check className="w-4 h-4 text-[#39ff14]" />
                ) : null}
                {settingActive ? "Estableciendo..." : activeSuccess ? "Activo" : "Establecer"}
              </button>
            </div>
          </div>

          {/* Pull New Model */}
          <form onSubmit={handlePullModel} className="space-y-3 pt-2 border-t border-white/5">
            <label htmlFor="pull-input" className="text-[10px] uppercase text-white/50 tracking-widest block font-medium">
              Descargar nuevo modelo (Pull)
            </label>
            <div className="flex gap-2">
              <input
                id="pull-input"
                type="text"
                value={pullModelName}
                onChange={(e) => setPullModelName(e.target.value)}
                placeholder="ej: deepseek-coder:1.5b"
                disabled={pulling}
                className="flex-1 bg-black/60 border border-white/10 rounded-xl px-4 py-2.5 text-xs font-mono text-white/90 outline-none focus:border-[#39ff14]/40 placeholder:text-white/20"
              />
              <button
                type="submit"
                disabled={pulling || !pullModelName.trim()}
                className="px-4 py-2.5 bg-white/5 hover:bg-[#39ff14]/10 border border-white/10 hover:border-[#39ff14]/30 hover:text-[#39ff14] text-white/80 font-bold rounded-xl text-xs transition-all flex items-center justify-center gap-2 shrink-0 disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-white/80"
              >
                {pulling ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : pullSuccess ? (
                  <Check className="w-4 h-4 text-[#39ff14]" />
                ) : (
                  <Download className="w-4 h-4" />
                )}
                {pulling ? "Descargando..." : pullSuccess ? "Descargado" : "Descargar"}
              </button>
            </div>
            {pullError && (
              <p className="text-[11px] text-red-400 font-medium">{pullError}</p>
            )}
          </form>
        </div>
      )}
    </div>
  );
}
