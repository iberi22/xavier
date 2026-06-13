import {
  Globe,
  RefreshCw,
  Plus,
  Trash2,
  CheckCircle2,
  AlertTriangle,
  Copy,
  Server,
  Activity,
  Shield,
  Network as NetworkIcon,
} from "lucide-react";
import { motion, AnimatePresence } from "motion/react";
import { useState, useEffect, useCallback } from "react";

interface Peer {
  node_id: string;
  alias: string | null;
  endpoint_url: string;
  public_key_hex: string;
  added_at: number;
  last_seen_at: number | null;
  sync_enabled: boolean;
}

interface LocalNodeInfo {
  node_id: string;
  public_key_hex: string;
}

const getApiUrl = (path: string) => {
  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  return isTauri ? `http://127.0.0.1:8006${path}` : path;
};

export default function NetworkPage({ token }: { token: string }) {
  const [localInfo, setLocalInfo] = useState<LocalNodeInfo | null>(null);
  const [peers, setPeers] = useState<Peer[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isAddingPeer, setIsAddingPeer] = useState(false);
  const [newPeerUrl, setNewPeerUrl] = useState("");
  const [newPeerAlias, setNewPeerAlias] = useState("");

  const fetchNetworkData = useCallback(async () => {
    try {
      setIsLoading(true);
      const [idResp, peersResp] = await Promise.all([
        fetch(getApiUrl("/panel/api/mesh/id"), {
          headers: { "X-Xavier-Token": token },
        }),
        fetch(getApiUrl("/panel/api/mesh/peers"), {
          headers: { "X-Xavier-Token": token },
        }),
      ]);

      if (!idResp.ok || !peersResp.ok) {
        throw new Error("Failed to fetch network data");
      }

      setLocalInfo(await idResp.json());
      setPeers(await peersResp.json());
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Connection error");
    } finally {
      setIsLoading(false);
    }
  }, [token]);

  useEffect(() => {
    fetchNetworkData();
  }, [fetchNetworkData]);

  const handleAddPeer = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      const resp = await fetch(getApiUrl("/panel/api/mesh/peers"), {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Xavier-Token": token,
        },
        body: JSON.stringify({ url: newPeerUrl, alias: newPeerAlias }),
      });

      if (!resp.ok) {
        const data = await resp.json();
        throw new Error(data.error || "Failed to add peer");
      }

      setIsAddingPeer(false);
      setNewPeerUrl("");
      setNewPeerAlias("");
      fetchNetworkData();
    } catch (e) {
      alert(e instanceof Error ? e.message : "Error adding peer");
    }
  };

  const handleRemovePeer = async (nodeId: string) => {
    if (!confirm("Are you sure you want to remove this peer?")) return;
    try {
      const resp = await fetch(getApiUrl(`/panel/api/mesh/peers/${nodeId}`), {
        method: "DELETE",
        headers: { "X-Xavier-Token": token },
      });

      if (!resp.ok) throw new Error("Failed to remove peer");
      fetchNetworkData();
    } catch (e) {
      alert(e instanceof Error ? e.message : "Error removing peer");
    }
  };

  if (isLoading && !localInfo) {
    return (
      <div className="flex items-center justify-center h-64">
        <RefreshCw className="w-6 h-6 text-[#39ff14] animate-spin" />
      </div>
    );
  }

  return (
    <div className="space-y-8 max-w-4xl">
      {/* Local Node Identity */}
      <section className="bg-white/[0.02] border border-white/[0.05] rounded-2xl p-6">
        <div className="flex items-center gap-3 mb-6">
          <div className="w-10 h-10 rounded-xl bg-[#39ff14]/10 flex items-center justify-center">
            <Server className="w-5 h-5 text-[#39ff14]" />
          </div>
          <div>
            <h2 className="text-lg font-medium text-white/90">Local Node Identity</h2>
            <p className="text-xs text-white/40">Your unique cryptographic ID in the Xavier Mesh</p>
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div className="p-4 rounded-xl bg-black/40 border border-white/[0.03]">
            <label className="text-[10px] uppercase tracking-widest text-white/30 mb-2 block">Node ID</label>
            <div className="flex items-center justify-between gap-2">
              <code className="text-xs font-mono text-[#39ff14]/80 truncate">{localInfo?.node_id}</code>
              <button
                onClick={() => navigator.clipboard.writeText(localInfo?.node_id || "")}
                className="p-1.5 hover:bg-white/5 rounded-lg text-white/30 hover:text-white/60 transition-all"
              >
                <Copy className="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
          <div className="p-4 rounded-xl bg-black/40 border border-white/[0.03]">
            <label className="text-[10px] uppercase tracking-widest text-white/30 mb-2 block">Public Key (HEX)</label>
            <div className="flex items-center justify-between gap-2">
              <code className="text-xs font-mono text-white/40 truncate">{localInfo?.public_key_hex}</code>
              <button
                onClick={() => navigator.clipboard.writeText(localInfo?.public_key_hex || "")}
                className="p-1.5 hover:bg-white/5 rounded-lg text-white/30 hover:text-white/60 transition-all"
              >
                <Copy className="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
        </div>
      </section>

      {/* Peers List */}
      <section className="space-y-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <NetworkIcon className="w-4 h-4 text-white/60" />
            <h2 className="text-sm font-medium text-white/80">Connected Peers</h2>
          </div>
          <button
            onClick={() => setIsAddingPeer(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 bg-[#39ff14]/10 border border-[#39ff14]/20 text-[#39ff14] text-xs rounded-lg hover:bg-[#39ff14]/20 transition-all"
          >
            <Plus className="w-3.5 h-3.5" />
            Link New Node
          </button>
        </div>

        {error && (
          <div className="p-4 rounded-xl bg-red-500/10 border border-red-500/20 flex items-center gap-3 text-red-400 text-sm">
            <AlertTriangle className="w-4 h-4" />
            {error}
          </div>
        )}

        <AnimatePresence>
          {isAddingPeer && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              className="overflow-hidden"
            >
              <form onSubmit={handleAddPeer} className="p-5 rounded-2xl bg-[#39ff14]/[0.02] border border-[#39ff14]/10 space-y-4 mb-4">
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <div>
                    <label className="text-[10px] uppercase tracking-widest text-white/40 mb-1.5 block">Endpoint URL</label>
                    <input
                      type="url"
                      required
                      placeholder="http://192.168.1.50:8006"
                      value={newPeerUrl}
                      onChange={e => setNewPeerUrl(e.target.value)}
                      className="w-full bg-black/40 border border-white/10 rounded-xl px-4 py-2.5 text-sm text-white/80 outline-none focus:border-[#39ff14]/40 transition-all"
                    />
                  </div>
                  <div>
                    <label className="text-[10px] uppercase tracking-widest text-white/40 mb-1.5 block">Alias (Optional)</label>
                    <input
                      type="text"
                      placeholder="Laptop-Pro"
                      value={newPeerAlias}
                      onChange={e => setNewPeerAlias(e.target.value)}
                      className="w-full bg-black/40 border border-white/10 rounded-xl px-4 py-2.5 text-sm text-white/80 outline-none focus:border-[#39ff14]/40 transition-all"
                    />
                  </div>
                </div>
                <div className="flex gap-2">
                  <button type="submit" className="flex-1 bg-[#39ff14]/10 border border-[#39ff14]/30 text-[#39ff14] py-2.5 rounded-xl text-sm font-medium hover:bg-[#39ff14]/20 transition-all">
                    Initialize Handshake
                  </button>
                  <button
                    type="button"
                    onClick={() => setIsAddingPeer(false)}
                    className="px-6 py-2.5 bg-white/5 border border-white/10 text-white/60 rounded-xl text-sm hover:bg-white/10 transition-all"
                  >
                    Cancel
                  </button>
                </div>
                <p className="text-[10px] text-white/30 leading-relaxed italic">
                  Note: Handshake requires the remote node to be reachable and using the same XMesh protocol version.
                </p>
              </form>
            </motion.div>
          )}
        </AnimatePresence>

        <div className="space-y-3">
          {peers.length === 0 ? (
            <div className="py-12 flex flex-col items-center justify-center bg-white/[0.01] border border-dashed border-white/10 rounded-2xl">
              <Globe className="w-8 h-8 text-white/10 mb-3" />
              <p className="text-sm text-white/30">No trusted peers connected</p>
              <button
                onClick={() => setIsAddingPeer(true)}
                className="mt-4 text-xs text-[#39ff14] hover:underline"
              >
                Add your first node
              </button>
            </div>
          ) : (
            peers.map(peer => (
              <div key={peer.node_id} className="group p-4 rounded-2xl bg-white/[0.02] border border-white/[0.05] hover:border-white/10 transition-all">
                <div className="flex items-start justify-between">
                  <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-xl bg-black/40 flex items-center justify-center text-white/20">
                      <Globe className="w-5 h-5" />
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <h3 className="text-sm font-medium text-white/90">{peer.alias || "Unnamed Node"}</h3>
                        <span className={`w-1.5 h-1.5 rounded-full ${peer.last_seen_at ? 'bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.6)]' : 'bg-white/10'}`} />
                      </div>
                      <p className="text-[10px] font-mono text-white/30 mt-0.5">{peer.node_id}</p>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => handleRemovePeer(peer.node_id)}
                      className="p-2 text-white/20 hover:text-red-400 hover:bg-red-500/10 rounded-lg transition-all"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                </div>

                <div className="mt-4 pt-4 border-t border-white/[0.03] flex items-center justify-between text-[10px]">
                  <div className="flex gap-4">
                    <div className="flex items-center gap-1.5 text-white/40">
                      <Activity className="w-3 h-3" />
                      <span>{peer.endpoint_url}</span>
                    </div>
                    <div className="flex items-center gap-1.5 text-white/40">
                      <Shield className="w-3 h-3" />
                      <span className="truncate max-w-[100px]">{peer.public_key_hex}</span>
                    </div>
                  </div>
                  <div className="text-white/20">
                    Added {new Date(peer.added_at * 1000).toLocaleDateString()}
                  </div>
                </div>
              </div>
            ))
          )}
        </div>
      </section>

      {/* Mesh Status Footer */}
      <div className="p-4 rounded-xl bg-black/20 border border-white/[0.03] flex items-center justify-between">
        <div className="flex items-center gap-2 text-xs text-white/40">
          <CheckCircle2 className="w-3.5 h-3.5 text-green-500/60" />
          XMesh Protocol v1 Active
        </div>
        <div className="text-[10px] text-white/20 uppercase tracking-widest">
          {peers.length} Trusted Peer{peers.length !== 1 && 's'}
        </div>
      </div>
    </div>
  );
}
