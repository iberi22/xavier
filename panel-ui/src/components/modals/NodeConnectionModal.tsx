import { Globe, RefreshCw, Server, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";
import { getApiUrl } from "../../api/client";

export interface NodeConnectionModalProps {
  isOpen: boolean;
  onClose: () => void;
  nodeStatus: "connected" | "disconnected" | "retrying";
  retryCount: number;
  remoteUrl: string;
  onSaveRemoteUrl: (url: string | null) => void;
}

export function NodeConnectionModal({
  isOpen,
  onClose,
  nodeStatus,
  retryCount,
  remoteUrl,
  onSaveRemoteUrl,
}: NodeConnectionModalProps) {
  const [inputRemoteUrl, setInputRemoteUrl] = useState<string>(remoteUrl);
  const [testingConnection, setTestingConnection] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null);

  useEffect(() => {
    if (isOpen) {
      setInputRemoteUrl(remoteUrl);
      setTestResult(null);
    }
  }, [isOpen, remoteUrl]);

  const handleTestConnection = async () => {
    setTestingConnection(true);
    setTestResult(null);
    try {
      const target = inputRemoteUrl.trim().replace(/\/+$/, "");
      const healthUrl = target ? `${target}/health` : getApiUrl("/health");
      const res = await fetch(healthUrl);
      if (res.ok) {
        setTestResult({ ok: true, msg: "Connected successfully to node!" });
      } else {
        setTestResult({
          ok: false,
          msg: `HTTP Error ${res.status}: ${res.statusText}`,
        });
      }
    } catch (_err) {
      setTestResult({
        ok: false,
        msg: "Connection failed. Check node URL, CORS policies or network availability.",
      });
    } finally {
      setTestingConnection(false);
    }
  };

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="fixed inset-0 z-[100] bg-black/70 backdrop-blur-md flex items-center justify-center p-4 pointer-events-auto"
          onClick={onClose}
        >
          <motion.div
            initial={{ scale: 0.95, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.95, opacity: 0 }}
            className="bg-[#0a0a0a] border border-white/10 rounded-2xl p-6 w-full max-w-md shadow-2xl space-y-5"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-white/10 pb-4">
              <div className="flex items-center gap-2">
                <Server className="w-5 h-5 text-emerald-400" />
                <div>
                  <h3 className="text-sm font-bold text-white uppercase tracking-wider font-mono">
                    Xavier Node Connectivity
                  </h3>
                  <p className="text-[10px] text-white/50">
                    Configure active remote node URL & auto-reconnect settings
                  </p>
                </div>
              </div>
              <button
                type="button"
                onClick={onClose}
                className="text-white/40 hover:text-white transition-colors"
                aria-label="Close modal"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="space-y-4">
              {/* Active Mode Banner */}
              <div className="bg-white/5 border border-white/10 p-3 rounded-xl flex items-center justify-between text-xs font-mono">
                <span className="text-white/60">Current Base URL:</span>
                <span
                  className="text-emerald-400 font-bold truncate max-w-[200px]"
                  title={getApiUrl("")}
                >
                  {getApiUrl("") || "Relative / Default"}
                </span>
              </div>

              {/* Status Indicator */}
              <div className="flex items-center gap-2 text-xs font-mono">
                <span className="text-white/60">Status:</span>
                <span
                  className={`px-2 py-0.5 rounded-full text-[10px] font-bold uppercase border ${
                    nodeStatus === "connected"
                      ? "bg-emerald-500/10 text-emerald-400 border-emerald-500/30"
                      : "bg-amber-500/10 text-amber-400 border-amber-500/30 animate-pulse"
                  }`}
                >
                  {nodeStatus === "connected"
                    ? "Connected"
                    : `Disconnected (${retryCount} retries)`}
                </span>
              </div>

              {/* Remote URL Input */}
              <div className="space-y-1.5">
                <label
                  htmlFor="remote-node-url-input"
                  className="block text-xs font-mono text-white/70"
                >
                  Remote Node Endpoint (e.g., https://xavier-node.domain.com:8006)
                </label>
                <div className="relative">
                  <Globe className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-white/40" />
                  <input
                    id="remote-node-url-input"
                    type="text"
                    value={inputRemoteUrl}
                    onChange={(e) => setInputRemoteUrl(e.target.value)}
                    placeholder="https://node.swal.local:8006"
                    className="w-full pl-9 pr-3 py-2 bg-black/50 border border-white/15 rounded-xl text-white text-xs font-mono focus:outline-none focus:border-emerald-400 transition-colors"
                  />
                </div>
              </div>

              {testResult && (
                <div
                  className={`p-3 rounded-xl border text-xs font-mono ${
                    testResult.ok
                      ? "bg-emerald-500/10 border-emerald-500/30 text-emerald-300"
                      : "bg-red-500/10 border-red-500/30 text-red-300"
                  }`}
                >
                  {testResult.msg}
                </div>
              )}

              {/* Button Controls */}
              <div className="flex items-center justify-between gap-2 pt-2 border-t border-white/10">
                <button
                  type="button"
                  onClick={handleTestConnection}
                  disabled={testingConnection}
                  className="px-3 py-1.5 bg-white/5 hover:bg-white/10 border border-white/10 text-white/80 rounded-xl text-xs font-mono font-bold transition-colors flex items-center gap-1.5 disabled:opacity-50"
                >
                  {testingConnection && (
                    <RefreshCw className="w-3 h-3 animate-spin" />
                  )}
                  Test
                </button>
                <div className="flex items-center gap-2">
                  {remoteUrl && (
                    <button
                      type="button"
                      onClick={() => onSaveRemoteUrl(null)}
                      className="px-3 py-1.5 bg-red-500/10 hover:bg-red-500/20 border border-red-500/30 text-red-300 rounded-xl text-xs font-mono font-bold transition-colors"
                    >
                      Use Local
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => onSaveRemoteUrl(inputRemoteUrl)}
                    className="px-4 py-1.5 bg-emerald-500 text-black hover:bg-emerald-400 rounded-xl text-xs font-mono font-bold transition-colors"
                  >
                    Save Remote Node
                  </button>
                </div>
              </div>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

export default NodeConnectionModal;
