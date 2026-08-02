import {
  AlertCircle,
  Check,
  Download,
  Folder,
  Loader2,
  Plus,
  RefreshCw,
  Trash2,
  Power,
} from "lucide-react";
import React, { useCallback, useEffect, useState } from "react";
import { ApiClient } from "../api/client";

interface OfflineModelManagerProps {
  token: string;
}

interface OfflineModel {
  name: string;
  path: string;
  size_bytes: number;
  quantization: string | null;
}

interface OfflineStatus {
  gpu_detected: boolean;
  gpu_vendor: string;
  vram_mb: number;
  engine_status: string;
  active_model: string;
  port: number;
}

export function OfflineModelManager({ token }: OfflineModelManagerProps) {
  const [client] = useState(() => new ApiClient(token));
  const [localDirs, setLocalDirs] = useState<string[]>([]);
  const [newDir, setNewDir] = useState<string>("");
  const [autoStart, setAutoStart] = useState<boolean>(false);
  const [models, setModels] = useState<OfflineModel[]>([]);
  const [status, setStatus] = useState<OfflineStatus | null>(null);
  const [downloadUrl, setDownloadUrl] = useState<string>("");

  const [loading, setLoading] = useState<boolean>(true);
  const [savingConfig, setSavingConfig] = useState<boolean>(false);
  const [downloading, setDownloading] = useState<boolean>(false);

  const [error, setError] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  const loadData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [configResp, modelsResp, statusResp] = await Promise.all([
        client.getOfflineConfig(),
        client.getOfflineModels(),
        client.getOfflineStatus(),
      ]);

      setLocalDirs(configResp.local_model_dirs || []);
      setAutoStart(configResp.auto_start_last_model || false);
      setModels(modelsResp.models || []);
      setStatus(statusResp);
    } catch (e: any) {
      console.error("Failed to load offline models data:", e);
      setError("No se pudo conectar con el servicio de modelos offline.");
    } finally {
      setLoading(false);
    }
  }, [client]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleSaveConfig = async (
    updatedDirs: string[],
    updatedAutoStart: boolean,
  ) => {
    setSavingConfig(true);
    setError(null);
    try {
      await client.updateOfflineConfig({
        local_model_dirs: updatedDirs,
        auto_start_last_model: updatedAutoStart,
      });
      setSuccessMsg("Configuración guardada correctamente.");
      setTimeout(() => setSuccessMsg(null), 3000);
      await loadData();
    } catch (e: any) {
      setError(e.message || "Error al guardar la configuración.");
    } finally {
      setSavingConfig(false);
    }
  };

  const handleAddDir = (e: React.FormEvent) => {
    e.preventDefault();
    if (!newDir.trim()) return;
    const cleanDir = newDir.trim();
    if (localDirs.includes(cleanDir)) {
      setNewDir("");
      return;
    }
    const updated = [...localDirs, cleanDir];
    setLocalDirs(updated);
    setNewDir("");
    handleSaveConfig(updated, autoStart);
  };

  const handleRemoveDir = (dirToRemove: string) => {
    const updated = localDirs.filter((d) => d !== dirToRemove);
    setLocalDirs(updated);
    handleSaveConfig(updated, autoStart);
  };

  const handleToggleAutoStart = () => {
    const nextVal = !autoStart;
    setAutoStart(nextVal);
    handleSaveConfig(localDirs, nextVal);
  };

  const handleDownloadModel = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!downloadUrl.trim()) return;
    setDownloading(true);
    setError(null);
    setSuccessMsg(null);
    try {
      const resp = await client.downloadOfflineModel(downloadUrl.trim());
      setSuccessMsg(`Descargado con éxito: ${resp.filename}`);
      setDownloadUrl("");
      setTimeout(() => setSuccessMsg(null), 4000);
      await loadData();
    } catch (e: any) {
      setError(e.message || "Error al descargar el modelo.");
    } finally {
      setDownloading(false);
    }
  };

  const formatSize = (bytes: number): string => {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
  };

  return (
    <div className="bg-[#050505]/80 border border-white/10 rounded-[24px] p-6 backdrop-blur-xl shadow-2xl space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <div className="relative">
            <div
              className={`w-2.5 h-2.5 rounded-full ${error ? "bg-red-500" : "bg-[#39ff14]"}`}
            />
            {!error && (
              <div className="absolute inset-0 w-2.5 h-2.5 rounded-full bg-[#39ff14] animate-ping opacity-75" />
            )}
          </div>
          <h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/80">
            Offline Model Manager (Local GGUF Engine)
          </h3>
        </div>
        <button
          onClick={loadData}
          disabled={loading || downloading || savingConfig}
          aria-label="Refrescar modelos offline"
          className="p-1.5 hover:bg-white/5 rounded-lg transition-colors text-white/40 hover:text-white/80 disabled:opacity-30"
        >
          <RefreshCw className={`w-4 h-4 ${loading ? "animate-spin" : ""}`} />
        </button>
      </div>

      {error && (
        <div className="flex items-start gap-3 bg-red-950/30 border border-red-500/20 rounded-xl p-4 text-red-400">
          <AlertCircle className="w-5 h-5 shrink-0 mt-0.5" />
          <p className="text-xs font-semibold">{error}</p>
        </div>
      )}

      {successMsg && (
        <div className="flex items-start gap-3 bg-emerald-950/30 border border-emerald-500/20 rounded-xl p-4 text-emerald-400">
          <Check className="w-5 h-5 shrink-0 mt-0.5" />
          <p className="text-xs font-semibold">{successMsg}</p>
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Left Side: Server Status & Auto-Start & Folders */}
        <div className="space-y-6">
          {/* Hardware & Server Status */}
          <div className="bg-white/5 border border-white/5 rounded-xl p-4 space-y-3">
            <h4 className="text-[10px] uppercase text-white/50 tracking-widest font-bold">
              Estado del Motor
            </h4>
            {status ? (
              <div className="grid grid-cols-2 gap-3 text-xs">
                <div>
                  <span className="text-white/40 block mb-0.5">CPU/GPU</span>
                  <span className="font-semibold text-white/90 truncate block">
                    {status.gpu_vendor}
                  </span>
                </div>
                <div>
                  <span className="text-white/40 block mb-0.5">
                    VRAM Disponible
                  </span>
                  <span className="font-semibold text-white/90 truncate block">
                    {status.gpu_detected ? `${status.vram_mb} MB` : "N/A"}
                  </span>
                </div>
                <div>
                  <span className="text-white/40 block mb-0.5">
                    Puerto Servidor
                  </span>
                  <span className="font-mono text-white/90 block">
                    :{status.port}
                  </span>
                </div>
                <div>
                  <span className="text-white/40 block mb-0.5">Estado</span>
                  <span className="font-semibold text-[#39ff14] uppercase tracking-wider block">
                    {status.engine_status}
                  </span>
                </div>
              </div>
            ) : (
              <div className="flex items-center gap-2 text-white/40 text-xs">
                <Loader2 className="w-4 h-4 animate-spin text-[#39ff14]" />
                Detectando hardware...
              </div>
            )}
          </div>

          {/* Auto-Start Switch */}
          <div className="flex items-center justify-between p-4 rounded-xl bg-white/5 border border-white/5 hover:border-white/10 transition-all">
            <div>
              <h4 className="text-white/90 text-xs font-semibold mb-0.5 flex items-center gap-2">
                <Power className="w-3.5 h-3.5 text-[#39ff14]" />
                Auto-Start (Arranque Automático)
              </h4>
              <p className="text-[10px] text-white/40">
                Inicia el último modelo usado automáticamente al iniciar Xavier.
              </p>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={autoStart}
              onClick={handleToggleAutoStart}
              disabled={savingConfig}
              className={`relative w-10 h-6 rounded-full transition-all duration-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]/50 ${
                autoStart ? "bg-[#39ff14]" : "bg-white/10"
              }`}
              aria-label="Toggle auto start"
            >
              <div
                className={`absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform duration-300 ${
                  autoStart ? "translate-x-4" : "translate-x-0 opacity-70"
                }`}
              />
            </button>
          </div>

          {/* Local Directories Settings */}
          <div className="space-y-3">
            <h4 className="text-[10px] uppercase text-white/50 tracking-widest font-bold">
              Directorios de Búsqueda
            </h4>

            <form onSubmit={handleAddDir} className="flex gap-2">
              <input
                type="text"
                value={newDir}
                onChange={(e) => setNewDir(e.target.value)}
                placeholder="ej: /Users/xavier/models"
                aria-label="Añadir directorio adicional"
                className="flex-1 bg-black/60 border border-white/10 rounded-xl px-4 py-2 text-xs font-mono text-white/90 outline-none focus:border-[#39ff14]/40 placeholder:text-white/20"
              />
              <button
                type="submit"
                disabled={savingConfig || !newDir.trim()}
                aria-label="Añadir carpeta"
                className="px-3 py-2 bg-white/5 hover:bg-[#39ff14]/10 border border-white/10 hover:border-[#39ff14]/30 text-white hover:text-[#39ff14] rounded-xl text-xs transition-all flex items-center justify-center gap-1 shrink-0 disabled:opacity-40"
              >
                <Plus className="w-4 h-4" />
              </button>
            </form>

            <div className="space-y-2 max-h-36 overflow-y-auto">
              {localDirs.length === 0 ? (
                <p className="text-[10px] text-white/30 italic">
                  No hay directorios adicionales configurados. (Se usará la ruta
                  por defecto).
                </p>
              ) : (
                localDirs.map((dir) => (
                  <div
                    key={dir}
                    className="flex items-center justify-between bg-black/40 border border-white/5 p-2 rounded-xl"
                  >
                    <span className="text-[11px] font-mono text-white/80 truncate pr-2 flex items-center gap-1.5">
                      <Folder className="w-3.5 h-3.5 text-white/40 shrink-0" />
                      {dir}
                    </span>
                    <button
                      type="button"
                      onClick={() => handleRemoveDir(dir)}
                      aria-label={`Eliminar directorio ${dir}`}
                      className="p-1 text-white/40 hover:text-red-400 rounded-lg hover:bg-red-500/10 transition-all"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>

        {/* Right Side: HuggingFace Download & Discovered Models */}
        <div className="space-y-6">
          {/* HuggingFace Downloader */}
          <form onSubmit={handleDownloadModel} className="space-y-3">
            <h4 className="text-[10px] uppercase text-white/50 tracking-widest font-bold">
              Descargar desde HuggingFace
            </h4>
            <div className="flex gap-2">
              <input
                type="text"
                value={downloadUrl}
                onChange={(e) => setDownloadUrl(e.target.value)}
                placeholder="Pegar URL directa de GGUF o HuggingFace"
                aria-label="URL HuggingFace de GGUF"
                className="flex-1 bg-black/60 border border-white/10 rounded-xl px-4 py-2.5 text-xs font-mono text-white/90 outline-none focus:border-[#39ff14]/40 placeholder:text-white/20"
              />
              <button
                type="submit"
                disabled={downloading || !downloadUrl.trim()}
                className="px-4 py-2.5 bg-white/5 hover:bg-[#39ff14]/10 border border-white/10 hover:border-[#39ff14]/30 hover:text-[#39ff14] text-white/80 font-bold rounded-xl text-xs transition-all flex items-center justify-center gap-2 shrink-0 disabled:opacity-40"
              >
                {downloading ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Download className="w-4 h-4" />
                )}
                {downloading ? "Descargando..." : "Descargar"}
              </button>
            </div>
          </form>

          {/* Discovered Models */}
          <div className="space-y-3">
            <h4 className="text-[10px] uppercase text-white/50 tracking-widest font-bold">
              Modelos Descubiertos (.gguf)
            </h4>
            <div className="space-y-2 max-h-56 overflow-y-auto">
              {models.length === 0 ? (
                <div className="border border-dashed border-white/10 p-6 rounded-xl text-center text-xs text-white/30 italic">
                  No se encontraron archivos GGUF en las carpetas especificadas.
                </div>
              ) : (
                models.map((model) => (
                  <div
                    key={model.path}
                    className="bg-white/5 border border-white/5 p-3 rounded-xl hover:border-white/10 transition-all flex flex-col gap-1"
                  >
                    <div className="flex items-center justify-between">
                      <span className="text-xs font-bold text-white/90 truncate pr-2">
                        {model.name}
                      </span>
                      <span className="text-[10px] bg-[#39ff14]/10 border border-[#39ff14]/20 text-[#39ff14] px-1.5 py-0.5 rounded font-bold uppercase shrink-0">
                        {model.quantization || "GGUF"}
                      </span>
                    </div>
                    <div className="flex justify-between items-center text-[10px] text-white/40 font-mono">
                      <span className="truncate pr-4" title={model.path}>
                        {model.path}
                      </span>
                      <span className="shrink-0">
                        {formatSize(model.size_bytes)}
                      </span>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
