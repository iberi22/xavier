import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  Copy,
  Link,
  Plus,
  RefreshCw,
  Shield,
  Trash2,
  UserPlus,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import { ApiClient } from "../api/client";
import type { ClearanceLevel, MeshPeer, MeshRole } from "../types";

interface MeshConfigProps {
  token: string;
}

export default function MeshConfig({ token }: MeshConfigProps) {
  const [localNodeId, setLocalNodeId] = useState<string>("");
  const [peers, setPeers] = useState<MeshPeer[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [pairingCode, setPairingCode] = useState("");
  const [isPairing, setIsPairing] = useState(false);
  const [generatedCode, setGeneratedCode] = useState<{
    code: string;
    secret: string;
  } | null>(null);

  const client = new ApiClient(token);

  const loadData = useCallback(async () => {
    try {
      setIsLoading(true);
      const status = await client.getMeshStatus();
      setPeers(status.peers);
      setLocalNodeId(status.local_node_id);
      setError(null);
    } catch (e) {
      setError("Failed to load mesh status");
      console.error(e);
    } finally {
      setIsLoading(false);
    }
  }, [token]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handlePair = async () => {
    if (!pairingCode.trim()) return;
    try {
      setIsPairing(true);
      await client.pairPeer(pairingCode.trim());
      setPairingCode("");
      loadData();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to pair with node");
    } finally {
      setIsPairing(false);
    }
  };

  const handleGenerateCode = async () => {
    try {
      const resp = await client.generatePairingCode();
      setGeneratedCode(resp);
    } catch (e) {
      setError("Failed to generate pairing code");
    }
  };

  const handleUpdateAcl = async (
    nodeId: string,
    role: MeshRole,
    clearance: ClearanceLevel,
  ) => {
    try {
      await client.updatePeerAcl(nodeId, role, clearance);
      loadData();
    } catch (e) {
      setError("Failed to update peer permissions");
    }
  };

  const handleRemovePeer = async (nodeId: string) => {
    if (!confirm("Are you sure you want to revoke this peer?")) return;
    try {
      await client.removePeer(nodeId);
      loadData();
    } catch (e) {
      setError("Failed to remove peer");
    }
  };

  return (
    <div className="space-y-8 p-6 overflow-y-auto h-full">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-light text-white tracking-tight">
            Mesh Network & Sharing
          </h2>
          <p className="text-sm text-white/40 mt-1">
            Connect multiple Xavier instances and manage data sharing permissions.
          </p>
        </div>
        <div className="text-right">
          <p className="text-[10px] uppercase text-white/30 tracking-widest">
            Local Node ID
          </p>
          <code className="text-xs text-[#39ff14] font-mono select-all">
            {localNodeId || "loading..."}
          </code>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Pairing Actions */}
        <section className="space-y-4">
          <h3 className="text-sm font-medium text-white/80">Link New Node</h3>
          <div className="p-5 rounded-2xl bg-white/[0.02] border border-white/[0.06] space-y-4">
            <div className="space-y-2">
              <label className="text-[10px] uppercase tracking-widest text-white/40 block">
                Enter Pairing Code
              </label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={pairingCode}
                  onChange={(e) => setPairingCode(e.target.value)}
                  placeholder="Paste code from another node"
                  className="flex-1 bg-black/30 border border-white/10 text-white/80 text-xs px-3 py-2 rounded-lg outline-none focus:border-[#39ff14]/30 transition-all font-mono"
                />
                <button
                  onClick={handlePair}
                  disabled={isPairing || !pairingCode}
                  className="px-4 py-2 bg-[#39ff14]/10 border border-[#39ff14]/20 text-[#39ff14] text-xs rounded-lg hover:bg-[#39ff14]/20 disabled:opacity-50 transition-all flex items-center gap-2"
                >
                  {isPairing ? (
                    <RefreshCw className="w-3 h-3 animate-spin" />
                  ) : (
                    <Link className="w-3 h-3" />
                  )}
                  Join
                </button>
              </div>
            </div>

            <div className="pt-4 border-t border-white/[0.04] space-y-4">
              <div className="flex items-center justify-between">
                <p className="text-xs text-white/60">Generate my pairing code</p>
                <button
                  onClick={handleGenerateCode}
                  className="flex items-center gap-1.5 px-3 py-1.5 text-xs text-white/70 border border-white/10 rounded-lg hover:border-white/20 transition-all"
                >
                  <UserPlus className="w-3.5 h-3.5" />
                  Generate
                </button>
              </div>

              <AnimatePresence>
                {generatedCode && (
                  <motion.div
                    initial={{ opacity: 0, y: 10 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: 10 }}
                    className="p-4 rounded-xl bg-[#39ff14]/[0.03] border border-[#39ff14]/10 space-y-3"
                  >
                    <div>
                      <label className="text-[9px] uppercase tracking-widest text-[#39ff14]/60 block mb-1">
                        Pairing Code (Share with peer)
                      </label>
                      <div className="flex gap-2">
                        <code className="flex-1 bg-black/40 p-2 rounded text-[10px] text-white/80 font-mono break-all line-clamp-2">
                          {generatedCode.code}
                        </code>
                        <button
                          onClick={() =>
                            navigator.clipboard.writeText(generatedCode.code)
                          }
                          className="p-2 text-white/40 hover:text-white transition-colors"
                        >
                          <Copy className="w-4 h-4" />
                        </button>
                      </div>
                    </div>
                    <div>
                      <label className="text-[9px] uppercase tracking-widest text-amber-400/60 block mb-1">
                        Verification Secret (Share separately)
                      </label>
                      <code className="text-xs text-amber-200/70 font-mono">
                        {generatedCode.secret}
                      </code>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          </div>
        </section>

        {/* Global Stats/Status */}
        <section className="space-y-4">
          <h3 className="text-sm font-medium text-white/80">Network Status</h3>
          <div className="p-5 rounded-2xl bg-white/[0.02] border border-white/[0.06] flex flex-col justify-center gap-6 h-[calc(100%-2rem)]">
            <div className="flex items-center gap-4">
              <div className="w-10 h-10 rounded-full bg-[#39ff14]/10 flex items-center justify-center">
                <Shield className="w-5 h-5 text-[#39ff14]" />
              </div>
              <div>
                <p className="text-sm text-white/80 font-medium">
                  Secure P2P Mesh
                </p>
                <p className="text-xs text-white/40">
                  {peers.length} Trusted Peer{peers.length !== 1 ? "s" : ""}
                </p>
              </div>
            </div>
            <div className="space-y-2">
              <div className="flex justify-between text-[10px] uppercase tracking-widest text-white/30">
                <span>Data Sovereignty</span>
                <span className="text-[#39ff14]">Active</span>
              </div>
              <div className="h-1.5 w-full bg-white/5 rounded-full overflow-hidden">
                <div className="h-full bg-[#39ff14] w-full" />
              </div>
              <p className="text-[10px] text-white/20 italic">
                All data remains encrypted at rest. Sharing is opt-in per peer.
              </p>
            </div>
          </div>
        </section>
      </div>

      {/* Peers List */}
      <section className="space-y-4">
        <h3 className="text-sm font-medium text-white/80">Trusted Peers</h3>
        {error && (
          <div className="flex items-center gap-2 p-3 bg-red-500/10 border border-red-500/20 rounded-xl text-red-400 text-xs">
            <AlertTriangle className="w-4 h-4" />
            {error}
          </div>
        )}

        <div className="space-y-3">
          {peers.length === 0 && !isLoading && (
            <div className="text-center py-12 bg-white/[0.01] border border-dashed border-white/5 rounded-2xl">
              <p className="text-sm text-white/20">
                No peers linked yet. Generate a code to get started.
              </p>
            </div>
          )}

          {peers.map((peer) => (
            <div
              key={peer.node_id}
              className="p-5 rounded-2xl bg-white/[0.02] border border-white/[0.06] hover:border-white/10 transition-all group"
            >
              <div className="flex items-start justify-between gap-4">
                <div className="flex items-center gap-4">
                  <div className="w-10 h-10 rounded-xl bg-white/5 flex items-center justify-center text-white/40 font-bold">
                    {peer.alias?.[0] || peer.node_id[0].toUpperCase()}
                  </div>
                  <div>
                    <div className="flex items-center gap-2">
                      <p className="text-sm font-medium text-white/90">
                        {peer.alias || "Xavier Node"}
                      </p>
                      <span
                        className={`w-2 h-2 rounded-full ${peer.last_seen_at ? "bg-[#39ff14] shadow-[0_0_8px_#39ff14]" : "bg-white/10"}`}
                      />
                    </div>
                    <code className="text-[10px] text-white/30 font-mono">
                      {peer.node_id}
                    </code>
                  </div>
                </div>

                <div className="flex items-center gap-4">
                  <div className="text-right">
                    <p className="text-[10px] uppercase text-white/30 tracking-widest mb-1">
                      Clearance Depth
                    </p>
                    <select
                      value={peer.clearance}
                      onChange={(e) =>
                        handleUpdateAcl(
                          peer.node_id,
                          peer.role,
                          e.target.value as ClearanceLevel,
                        )
                      }
                      className="bg-black/40 border border-white/10 rounded px-2 py-1 text-[11px] text-white/70 outline-none focus:border-[#39ff14]/40"
                    >
                      <option value="unclassified">Unclassified</option>
                      <option value="confidential">Confidential</option>
                      <option value="secret">Secret</option>
                      <option value="top_secret">Top Secret</option>
                    </select>
                  </div>
                  <div className="text-right">
                    <p className="text-[10px] uppercase text-white/30 tracking-widest mb-1">
                      Role
                    </p>
                    <select
                      value={peer.role}
                      onChange={(e) =>
                        handleUpdateAcl(
                          peer.node_id,
                          e.target.value as MeshRole,
                          peer.clearance,
                        )
                      }
                      className="bg-black/40 border border-white/10 rounded px-2 py-1 text-[11px] text-white/70 outline-none focus:border-[#39ff14]/40"
                    >
                      <option value="reader">Reader</option>
                      <option value="editor">Editor</option>
                      <option value="admin">Admin</option>
                    </select>
                  </div>
                  <button
                    onClick={() => handleRemovePeer(peer.node_id)}
                    className="p-2 text-white/10 hover:text-red-400 hover:bg-red-400/10 rounded-lg transition-all opacity-0 group-hover:opacity-100"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </div>
              </div>

              <div className="mt-4 pt-4 border-t border-white/[0.04] flex items-center justify-between text-[10px] text-white/20">
                <div className="flex items-center gap-3">
                  <span className="flex items-center gap-1">
                    <Activity className="w-3 h-3" />
                    {peer.last_seen_at
                      ? `Active ${new Date(peer.last_seen_at * 1000).toLocaleString()}`
                      : "Never seen"}
                  </span>
                  <span>·</span>
                  <span>{peer.endpoint_url}</span>
                </div>
                {peer.sync_enabled ? (
                  <span className="text-[#39ff14]/60 flex items-center gap-1">
                    <CheckCircle2 className="w-3 h-3" />
                    Sync Active
                  </span>
                ) : (
                  <span>Sync Paused</span>
                )}
              </div>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
