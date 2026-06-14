import {
  Activity,
  CheckCircle2,
  Copy,
  ExternalLink,
  Info,
  Network,
  Plus,
  QrCode,
  RefreshCw,
  Settings2,
  Shield,
  ShieldCheck,
  Trash2,
  User,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";
import { ApiClient, type MeshIdentity, type PeerInfo } from "../../api/client";

interface MeshSettingsProps {
  token: string;
}

export default function MeshSettings({ token }: MeshSettingsProps) {
  const [client] = useState(() => new ApiClient(token));
  const [identity, setIdentity] = useState<MeshIdentity | null>(null);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [pairingCode, setPairingCode] = useState<{
    code: string;
    secret: string;
  } | null>(null);
  const [joinCode, setJoinCode] = useState("");
  const [joining, setJoining] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadData();
  }, []);

  const loadData = async () => {
    setLoading(true);
    try {
      const [idRes, peersRes] = await Promise.all([
        client.getMeshIdentity(),
        client.getPeers(),
      ]);
      setIdentity(idRes);
      setPeers(peersRes);
    } catch (err: any) {
      setError(err.message || "Failed to load Mesh data");
    } finally {
      setLoading(false);
    }
  };

  const handleGenerateCode = async () => {
    setGenerating(true);
    try {
      const res = await client.generatePairingCode();
      setPairingCode(res);
    } catch (err: any) {
      setError(err.message || "Failed to generate pairing code");
    } finally {
      setGenerating(false);
    }
  };

  const handleJoinMesh = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!joinCode.trim()) return;
    setJoining(true);
    try {
      await client.joinMesh(joinCode.trim());
      setJoinCode("");
      loadData();
    } catch (err: any) {
      setError(err.message || "Failed to join Mesh");
    } finally {
      setJoining(false);
    }
  };

  const handleRemovePeer = async (nodeId: string) => {
    try {
      await client.removePeer(nodeId);
      loadData();
    } catch (err: any) {
      setError(err.message || "Failed to remove peer");
    }
  };

  if (loading && !identity) {
    return (
      <div className="flex items-center justify-center h-full text-white/40 text-sm">
        <RefreshCw className="w-4 h-4 animate-spin mr-2" />
        Initialising Mesh Connection...
      </div>
    );
  }

  return (
    <div className="space-y-8 max-w-4xl">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-light text-white tracking-tight flex items-center gap-2">
            <Network className="w-6 h-6 text-[#39ff14]" />
            Xavier P2P Mesh
          </h2>
          <p className="text-sm text-white/40 mt-1">
            Secure, decentralized memory synchronization between trusted nodes.
          </p>
        </div>
        {identity && (
          <div className="flex flex-col items-end">
            <span className="text-[10px] uppercase tracking-widest text-white/30 mb-1">
              Local Node ID
            </span>
            <code className="text-xs bg-white/5 px-2 py-1 rounded border border-white/10 text-[#39ff14]/80 font-mono">
              {identity.node_id}
            </code>
          </div>
        )}
      </div>

      {error && (
        <div className="p-4 rounded-xl bg-red-500/10 border border-red-500/20 text-red-200 text-xs flex items-center gap-3">
          <Info className="w-4 h-4" />
          {error}
          <button
            onClick={() => setError(null)}
            className="ml-auto text-white/40 hover:text-white"
          >
            Dismiss
          </button>
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Pairing Section */}
        <section className="bg-white/[0.02] border border-white/[0.05] rounded-2xl p-6 flex flex-col gap-6">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg bg-blue-500/10 text-blue-400">
              <QrCode size={18} />
            </div>
            <h3 className="font-medium text-white/90">Node Pairing</h3>
          </div>

          <div className="space-y-4">
            <div>
              <p className="text-xs text-white/40 mb-3">
                Generate a temporary code to allow another node to join your
                mesh.
              </p>
              {!pairingCode ? (
                <button
                  onClick={handleGenerateCode}
                  disabled={generating}
                  className="w-full py-2.5 bg-blue-500/10 hover:bg-blue-500/20 text-blue-400 border border-blue-500/20 rounded-xl text-xs font-semibold tracking-wide transition-all"
                >
                  {generating ? "Generating..." : "Generate Pairing Code"}
                </button>
              ) : (
                <div className="space-y-3">
                  <div className="p-3 bg-black/40 border border-blue-500/30 rounded-xl">
                    <label className="text-[9px] uppercase tracking-widest text-blue-400/60 mb-1 block">
                      Pairing Code
                    </label>
                    <div className="flex items-center justify-between">
                      <code className="text-[10px] font-mono text-white/80 break-all select-all">
                        {pairingCode.code}
                      </code>
                      <button
                        onClick={() =>
                          navigator.clipboard.writeText(pairingCode.code)
                        }
                        className="p-1 hover:bg-white/10 rounded transition-colors"
                      >
                        <Copy size={12} className="text-white/40" />
                      </button>
                    </div>
                  </div>
                  <div className="p-3 bg-black/40 border border-amber-500/30 rounded-xl">
                    <label className="text-[9px] uppercase tracking-widest text-amber-400/60 mb-1 block">
                      Verification Secret (Share Privately)
                    </label>
                    <div className="flex items-center justify-between">
                      <code className="text-[10px] font-mono text-white/80">
                        {pairingCode.secret}
                      </code>
                      <button
                        onClick={() =>
                          navigator.clipboard.writeText(pairingCode.secret)
                        }
                        className="p-1 hover:bg-white/10 rounded transition-colors"
                      >
                        <Copy size={12} className="text-white/40" />
                      </button>
                    </div>
                  </div>
                  <button
                    onClick={() => setPairingCode(null)}
                    className="w-full text-[10px] text-white/30 hover:text-white/60 transition-colors"
                  >
                    Clear pairing info
                  </button>
                </div>
              )}
            </div>

            <div className="pt-4 border-t border-white/5">
              <p className="text-xs text-white/40 mb-3">
                Join an existing mesh by entering a pairing code.
              </p>
              <form onSubmit={handleJoinMesh} className="flex gap-2">
                <input
                  type="text"
                  value={joinCode}
                  onChange={(e) => setJoinCode(e.target.value)}
                  placeholder="Paste pairing code here..."
                  className="flex-1 bg-black/40 border border-white/10 rounded-xl px-4 py-2 text-xs font-mono outline-none focus:border-[#39ff14]/30 text-white/80"
                />
                <button
                  type="submit"
                  disabled={joining || !joinCode}
                  className="px-4 py-2 bg-[#39ff14]/10 hover:bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/20 rounded-xl text-xs font-bold transition-all disabled:opacity-50"
                >
                  {joining ? "Joining..." : "Join"}
                </button>
              </form>
            </div>
          </div>
        </section>

        {/* Identity Section */}
        <section className="bg-white/[0.02] border border-white/[0.05] rounded-2xl p-6 flex flex-col gap-6">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg bg-emerald-500/10 text-emerald-400">
              <ShieldCheck size={18} />
            </div>
            <h3 className="font-medium text-white/90">Identity & Security</h3>
          </div>

          <div className="space-y-5">
            <div>
              <label className="text-[10px] uppercase tracking-widest text-white/30 mb-2 block">
                Public Key (Hex)
              </label>
              <div className="bg-black/30 border border-white/5 rounded-xl p-3 font-mono text-[10px] text-white/50 break-all leading-relaxed">
                {identity?.public_key_hex}
              </div>
            </div>

            <div className="p-4 rounded-xl bg-blue-500/5 border border-blue-500/10 flex gap-3 items-start">
              <Shield className="w-4 h-4 text-blue-400 flex-shrink-0 mt-0.5" />
              <div className="space-y-1">
                <p className="text-xs text-blue-200/80 font-medium">
                  Cryptographic Trust
                </p>
                <p className="text-[10px] text-blue-200/50 leading-normal">
                  Connections are secured with Ed25519 signatures. Your private
                  key never leaves this device and is stored in the local
                  hardware vault.
                </p>
              </div>
            </div>
          </div>
        </section>
      </div>

      {/* Peers List */}
      <section className="space-y-4">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-medium text-white/80 flex items-center gap-2">
            Trusted Peers
            <span className="px-2 py-0.5 rounded-full bg-white/5 text-white/40 text-[10px]">
              {peers.length}
            </span>
          </h3>
          <button
            onClick={loadData}
            className="p-1.5 hover:bg-white/5 rounded-lg transition-all text-white/30 hover:text-white"
          >
            <RefreshCw size={14} />
          </button>
        </div>

        <div className="space-y-3">
          {peers.length === 0 ? (
            <div className="p-12 border border-dashed border-white/5 rounded-2xl flex flex-col items-center justify-center text-center">
              <User className="w-8 h-8 text-white/10 mb-3" />
              <p className="text-sm text-white/30">No trusted peers yet.</p>
              <p className="text-[10px] text-white/20 mt-1 max-w-[200px]">
                Pair with another Xavier node to start synchronising memories.
              </p>
            </div>
          ) : (
            peers.map((peer) => (
              <PeerRow
                key={peer.node_id}
                peer={peer}
                onRemove={() => handleRemovePeer(peer.node_id)}
              />
            ))
          )}
        </div>
      </section>

      {/* Network Observability (Placeholder) */}
      <section className="p-6 bg-black/40 border border-white/5 rounded-2xl">
        <div className="flex items-center justify-between mb-6">
          <div className="flex items-center gap-3">
            <Activity className="w-5 h-5 text-[#39ff14]/70" />
            <h3 className="font-medium text-white/90">Network Telemetry</h3>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-full bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.5)]" />
            <span className="text-[10px] text-white/40 uppercase tracking-widest font-bold">
              Protocol v1 Active
            </span>
          </div>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          <MetricBox label="Chunks Synced" value="1,248" trend="+12" />
          <MetricBox label="Global Reputation" value="0.942" trend="+0.01" />
          <MetricBox label="Network Latency" value="42ms" />
          <MetricBox label="Data Contributed" value="4.2 GB" />
        </div>
      </section>
    </div>
  );
}

function PeerRow({ peer, onRemove }: { peer: PeerInfo; onRemove: () => void }) {
  return (
    <div className="p-4 rounded-xl bg-white/[0.02] border border-white/[0.06] flex items-center justify-between group hover:bg-white/[0.04] transition-all">
      <div className="flex items-center gap-4">
        <div className="w-10 h-10 rounded-xl bg-white/5 flex items-center justify-center relative">
          <User className="w-5 h-5 text-white/30" />
          {peer.last_seen_at && (
            <span className="absolute -top-1 -right-1 w-3 h-3 bg-green-500 border-2 border-black rounded-full" />
          )}
        </div>
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-white/90">
              {peer.alias || peer.node_id.slice(0, 12) + "..."}
            </span>
            {peer.is_cloud && (
              <span className="px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-400 text-[8px] uppercase tracking-widest font-bold">
                Cloud
              </span>
            )}
          </div>
          <div className="flex items-center gap-3 text-[10px] text-white/30 font-mono">
            <span className="flex items-center gap-1">
              <ExternalLink size={10} />
              {peer.endpoint_url}
            </span>
            {peer.last_seen_at && (
              <span className="flex items-center gap-1">
                <Clock size={10} />
                Seen {new Date(peer.last_seen_at * 1000).toLocaleTimeString()}
              </span>
            )}
          </div>
        </div>
      </div>

      <div className="flex items-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
        <button className="p-2 hover:bg-white/10 rounded-lg text-white/40 hover:text-white transition-all">
          <Settings2 size={14} />
        </button>
        <button
          onClick={onRemove}
          className="p-2 hover:bg-red-500/10 rounded-lg text-white/20 hover:text-red-400 transition-all"
        >
          <Trash2 size={14} />
        </button>
      </div>
    </div>
  );
}

function MetricBox({
  label,
  value,
  trend,
}: {
  label: string;
  value: string;
  trend?: string;
}) {
  return (
    <div className="p-4 rounded-xl bg-black/20 border border-white/[0.03]">
      <p className="text-[10px] uppercase tracking-widest text-white/30 mb-1">
        {label}
      </p>
      <div className="flex items-baseline gap-2">
        <span className="text-lg font-light text-white">{value}</span>
        {trend && <span className="text-[10px] text-green-400">{trend}</span>}
      </div>
    </div>
  );
}

function Clock({ size }: { size: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="12" cy="12" r="10" />
      <polyline points="12 6 12 12 16 14" />
    </svg>
  );
}
