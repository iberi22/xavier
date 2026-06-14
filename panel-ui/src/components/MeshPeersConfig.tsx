import {
  Network,
  Plus,
  RefreshCw,
  Shield,
  Trash2,
  UserPlus,
  Wifi,
} from "lucide-react";
import React, { useEffect, useState } from "react";
import { ApiClient } from "../api/client";
import type { NodeAclEntry, PairingCodeData, PeerInfo } from "../types";

export function MeshPeersConfig({ token }: { token: string }) {
  const [client] = useState(() => new ApiClient(token));
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [pairingCode, setPairingCode] = useState<PairingCodeData | null>(null);
  const [showAddPeer, setShowAddPeer] = useState(false);
  const [newPeerCode, setNewPeerCode] = useState("");
  const [isAdding, setIsAdding] = useState(false);

  const fetchPeers = async () => {
    try {
      const data = await client.getPeers();
      setPeers(data);
    } catch (err) {
      console.error("Failed to fetch peers", err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchPeers();
  }, [client]);

  const handleGeneratePairing = async () => {
    try {
      // For now, use localhost or a placeholder if we don't know the external IP
      const res = await client.generatePairingCode("http://localhost:8006");
      setPairingCode(res);
    } catch (err) {
      console.error("Failed to generate pairing code", err);
    }
  };

  const handleAddPeer = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newPeerCode) return;
    setIsAdding(true);
    try {
      await client.addPeer({ pairing_code: newPeerCode });
      setNewPeerCode("");
      setShowAddPeer(false);
      fetchPeers();
    } catch (err) {
      console.error("Failed to add peer", err);
    } finally {
      setIsAdding(false);
    }
  };

  const handleRemovePeer = async (nodeId: string) => {
    if (!confirm("Are you sure you want to disconnect this node?")) return;
    try {
      await client.removePeer(nodeId);
      fetchPeers();
    } catch (err) {
      console.error("Failed to remove peer", err);
    }
  };

  const handleUpdateAcl = async (nodeId: string, role: any, clearance: any) => {
    try {
      await client.updatePeerAcl(nodeId, { role, clearance });
      fetchPeers();
    } catch (err) {
      console.error("Failed to update ACL", err);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="p-2 rounded-lg bg-cyan-500/10 text-cyan-400">
            <Network size={18} />
          </div>
          <div>
            <h2 className="font-semibold text-white/90">XMesh Peer-to-Peer</h2>
            <p className="text-xs text-white/40 uppercase tracking-widest mt-1">
              Synchronize memory across your devices
            </p>
          </div>
        </div>
        <div className="flex gap-2">
          <button
            onClick={fetchPeers}
            className="p-2 hover:bg-white/5 rounded-lg text-white/40 hover:text-white transition-colors"
          >
            <RefreshCw size={16} className={loading ? "animate-spin" : ""} />
          </button>
          <button
            onClick={() => setShowAddPeer(!showAddPeer)}
            className="flex items-center gap-2 px-3 py-1.5 bg-cyan-500/20 text-cyan-400 border border-cyan-500/30 rounded-lg text-xs font-bold uppercase tracking-widest hover:bg-cyan-500/30 transition-colors"
          >
            <Plus size={14} />
            Link Node
          </button>
        </div>
      </div>

      {showAddPeer && (
        <form
          onSubmit={handleAddPeer}
          className="p-4 rounded-xl bg-cyan-500/5 border border-cyan-500/20 space-y-3"
        >
          <div className="flex flex-col gap-1">
            <label className="text-[10px] uppercase text-cyan-400/70 tracking-widest">
              Enter Pairing Code
            </label>
            <input
              type="text"
              value={newPeerCode}
              onChange={(e) => setNewPeerCode(e.target.value)}
              placeholder="Paste pairing code from another node"
              className="w-full bg-black/40 border border-white/5 rounded-lg px-4 py-2 text-xs font-mono outline-none focus:border-cyan-500/30 text-white/80"
            />
          </div>
          <div className="flex justify-end gap-3">
            <button
              type="button"
              onClick={() => setShowAddPeer(false)}
              className="px-3 py-1.5 text-white/40 hover:text-white text-xs uppercase tracking-widest"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isAdding || !newPeerCode}
              className="px-4 py-1.5 bg-cyan-500 text-black rounded-lg text-xs font-bold uppercase tracking-widest hover:bg-cyan-400 transition-colors disabled:opacity-50"
            >
              {isAdding ? "Linking..." : "Link Now"}
            </button>
          </div>
        </form>
      )}

      <div className="grid gap-4">
        {peers.length === 0 && !loading ? (
          <div className="p-8 border border-dashed border-white/5 rounded-2xl flex flex-col items-center justify-center text-white/20">
            <Network size={32} className="mb-3 opacity-10" />
            <p className="text-sm">No peers linked yet</p>
          </div>
        ) : (
          peers.map((peer) => (
            <div
              key={peer.node_id[0]}
              className="p-4 rounded-xl bg-white/5 border border-white/5 flex items-center justify-between group"
            >
              <div className="flex items-center gap-4">
                <div className="p-2 rounded-full bg-cyan-500/10 text-cyan-400">
                  <Wifi size={16} />
                </div>
                <div>
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-white/90">
                      {peer.alias || "Xavier Node"}
                    </span>
                    {peer.is_cloud && (
                      <span className="text-[8px] bg-orange-500/20 text-orange-400 px-1.5 py-0.5 rounded uppercase font-bold tracking-tighter">
                        Cloud
                      </span>
                    )}
                  </div>
                  <div className="text-[10px] font-mono text-white/30 truncate max-w-[200px]">
                    {peer.node_id[0]}
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-6">
                <div className="flex flex-col gap-1">
                  <label className="text-[9px] uppercase text-white/30 tracking-tighter">
                    Permissions
                  </label>
                  <div className="flex gap-2">
                    <select
                      value="Reader"
                      onChange={(e) => handleUpdateAcl(peer.node_id[0], e.target.value, "Unclassified")}
                      className="bg-black/40 border border-white/10 rounded px-1.5 py-0.5 text-[10px] text-white/60 outline-none"
                    >
                      <option value="Admin">Admin</option>
                      <option value="Editor">Editor</option>
                      <option value="Reader">Reader</option>
                    </select>
                  </div>
                </div>
                <button
                  onClick={() => handleRemovePeer(peer.node_id[0])}
                  className="p-2 hover:bg-red-500/10 text-white/20 hover:text-red-400 transition-colors rounded-lg"
                >
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))
        )}
      </div>

      <div className="pt-6 border-t border-white/5">
        <h3 className="text-xs font-bold text-white/50 uppercase tracking-widest mb-4 flex items-center gap-2">
          <UserPlus size={14} />
          Invite to this node
        </h3>
        {!pairingCode ? (
          <button
            onClick={handleGeneratePairing}
            className="w-full py-3 bg-white/5 border border-white/10 rounded-xl text-xs text-white/60 hover:bg-white/10 hover:text-white transition-all uppercase tracking-[0.2em]"
          >
            Generate Pairing Code
          </button>
        ) : (
          <div className="space-y-3">
            <div className="p-4 bg-black/60 border border-[#39ff14]/30 rounded-xl relative overflow-hidden group">
              <div className="absolute top-0 left-0 w-1 h-full bg-[#39ff14]/50" />
              <p className="text-[10px] text-[#39ff14] uppercase font-bold mb-1">
                Your Pairing Code
              </p>
              <div className="font-mono text-xs text-white/90 break-all select-all cursor-pointer bg-white/5 p-2 rounded">
                {pairingCode.pairing_code}
              </div>
              <p className="text-[9px] text-white/30 mt-2">
                Expires in 60 minutes. Sharing this allows another node to request synchronization.
              </p>
            </div>
            <button
              onClick={() => setPairingCode(null)}
              className="text-[10px] text-white/30 hover:text-white transition-colors uppercase tracking-widest"
            >
              Reset Code
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
