import React, { useEffect, useState } from "react";
import { malocaApi, MeshSnapshot } from "../api";
import { Server, Activity, Key, Cpu, Network } from "lucide-react";

export function RegistryTab() {
  const [mesh, setMesh] = useState<MeshSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    malocaApi.getMesh()
      .then((data) => {
        if (mounted) setMesh(data);
      })
      .catch((err) => {
        if (mounted) setError(err.message);
      })
      .finally(() => {
        if (mounted) setLoading(false);
      });

    return () => { mounted = false; };
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-48">
        <div className="text-cyan-400 font-mono text-sm animate-pulse">Scanning Mesh Topology...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-rose-950/30 border border-rose-800/50 rounded-lg text-rose-300 text-xs font-mono">
        Topology Sync Error: {error}
      </div>
    );
  }

  if (!mesh) return null;

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <div className="glass-panel p-4 border border-[var(--border-subtle)] rounded-xl bg-[var(--surface)]">
          <div className="flex items-center gap-2 mb-2 text-[color:color-mix(in_srgb,var(--foreground)_50%,transparent)]">
            <Network size={14} className="text-cyan-400" />
            <span className="text-xs uppercase tracking-wider font-mono">Mode</span>
          </div>
          <div className="text-lg font-bold text-[var(--foreground)] capitalize">{mesh.mode}</div>
        </div>

        <div className="glass-panel p-4 border border-[var(--border-subtle)] rounded-xl bg-[var(--surface)] lg:col-span-2">
          <div className="flex items-center gap-2 mb-2 text-[color:color-mix(in_srgb,var(--foreground)_50%,transparent)]">
            <Key size={14} className="text-cyan-400" />
            <span className="text-xs uppercase tracking-wider font-mono">Genesis Node</span>
          </div>
          <div className="text-sm font-bold text-emerald-400 font-mono truncate">{mesh.genesis_node_id}</div>
        </div>

        <div className="glass-panel p-4 border border-[var(--border-subtle)] rounded-xl bg-[var(--surface)]">
          <div className="flex items-center gap-2 mb-2 text-[color:color-mix(in_srgb,var(--foreground)_50%,transparent)]">
            <Server size={14} className="text-cyan-400" />
            <span className="text-xs uppercase tracking-wider font-mono">Active Nodes</span>
          </div>
          <div className="text-2xl font-bold text-[var(--foreground)]">{mesh.nodes.filter(n => n.active).length} <span className="text-xs text-[color:color-mix(in_srgb,var(--foreground)_40%,transparent)] font-normal">/ {mesh.nodes.length}</span></div>
        </div>
      </div>

      <div className="glass-panel border border-[var(--border-subtle)] rounded-xl bg-[var(--surface)] overflow-hidden">
        <div className="p-4 border-b border-[var(--border-subtle)] bg-[color:color-mix(in_srgb,var(--foreground)_2%,transparent)]">
          <h3 className="text-sm font-semibold text-[var(--foreground)] flex items-center gap-2">
            <Cpu size={14} className="text-emerald-400" />
            Node Directory
          </h3>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm text-[color:color-mix(in_srgb,var(--foreground)_70%,transparent)]">
            <thead className="bg-[color:color-mix(in_srgb,var(--foreground)_2%,transparent)] text-xs uppercase text-[color:color-mix(in_srgb,var(--foreground)_40%,transparent)] font-mono">
              <tr>
                <th className="px-4 py-3 font-medium">Node ID</th>
                <th className="px-4 py-3 font-medium">Role</th>
                <th className="px-4 py-3 font-medium">Karma</th>
                <th className="px-4 py-3 font-medium">Status</th>
                <th className="px-4 py-3 font-medium">Note</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {mesh.nodes.map((node) => (
                <tr key={node.node_id} className="hover:bg-[color:color-mix(in_srgb,var(--foreground)_2%,transparent)] transition-colors">
                  <td className="px-4 py-3 font-mono text-xs text-emerald-400">{node.node_id}</td>
                  <td className="px-4 py-3">
                    <span className="px-2 py-0.5 rounded text-xs border border-[var(--border-subtle)] bg-[color:color-mix(in_srgb,var(--foreground)_5%,transparent)]">
                      {node.role}
                    </span>
                  </td>
                  <td className="px-4 py-3 font-mono text-cyan-400">{node.karma.toLocaleString()}</td>
                  <td className="px-4 py-3">
                    <span className={`flex items-center gap-1.5 text-xs ${node.active ? 'text-emerald-400' : 'text-rose-400'}`}>
                      <span className={`w-1.5 h-1.5 rounded-full ${node.active ? 'bg-emerald-400' : 'bg-rose-400'}`}></span>
                      {node.active ? 'Active' : 'Offline'}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-xs text-[color:color-mix(in_srgb,var(--foreground)_50%,transparent)]">{node.note}</td>
                </tr>
              ))}
              {mesh.nodes.length === 0 && (
                <tr>
                  <td colSpan={5} className="px-4 py-8 text-center text-xs text-[color:color-mix(in_srgb,var(--foreground)_40%,transparent)] italic">
                    No nodes registered in the mesh topology.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
