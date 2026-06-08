import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Loader2, CheckCircle2, AlertCircle } from 'lucide-react';
import { SystemInfo } from './OnboardingFlow';

export function SystemScanStep({ onNext }: { onNext: (info: SystemInfo) => void }) {
  const [status, setStatus] = useState<'scanning' | 'success' | 'error'>('scanning');
  const [errorMsg, setErrorMsg] = useState('');
  const [logs, setLogs] = useState<string[]>(['> Initiating deep system scan...']);

  useEffect(() => {
    const scan = async () => {
      try {
        await new Promise((r) => setTimeout(r, 1000)); // Visual delay
        setLogs((prev) => [...prev, '> Checking RAM capacity...']);
        await new Promise((r) => setTimeout(r, 800));
        setLogs((prev) => [...prev, '> Querying wmic for GPU accelerators...']);
        await new Promise((r) => setTimeout(r, 800));
        setLogs((prev) => [...prev, '> Scanning process list for sibling nodes...']);
        
        const info = await invoke<SystemInfo>('scan_system');
        
        setLogs((prev) => [
          ...prev, 
          `> RAM: ${info.total_ram_gb.toFixed(1)} GB`,
          `> Cores: ${info.cpu_cores}`,
          `> GPU Detected: ${info.has_gpu ? 'YES' : 'NO'}`,
          `> OpenClaw: ${info.openclaw_running ? 'FOUND' : 'MISSING'}`,
          `> Hermes: ${info.hermes_running ? 'FOUND' : 'MISSING'}`,
          '> Scan complete.'
        ]);
        
        setStatus('success');
        
        // Auto proceed after short delay
        setTimeout(() => {
          onNext(info);
        }, 2000);
      } catch (e) {
        setStatus('error');
        setErrorMsg(String(e));
      }
    };

    scan();
  }, [onNext]);

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="text-center space-y-2">
        <h2 className="text-2xl font-bold text-white">SYSTEM_DIAGNOSTICS</h2>
        <p className="text-emerald-500/70 text-sm">Analyzing environment constraints</p>
      </div>

      <div className="bg-black/50 border border-emerald-900/50 rounded p-4 font-mono text-sm h-64 overflow-y-auto">
        {logs.map((log, i) => (
          <div key={i} className="text-emerald-400/80 mb-1 animate-in slide-in-from-left-2">{log}</div>
        ))}
        
        {status === 'scanning' && (
          <div className="flex items-center gap-2 text-emerald-300 mt-4">
            <Loader2 className="w-4 h-4 animate-spin" /> Processing...
          </div>
        )}
        
        {status === 'success' && (
          <div className="flex items-center gap-2 text-emerald-400 mt-4 font-bold">
            <CheckCircle2 className="w-4 h-4" /> ANALYSIS_COMPLETE
          </div>
        )}

        {status === 'error' && (
          <div className="flex items-center gap-2 text-red-400 mt-4">
            <AlertCircle className="w-4 h-4" /> ERROR: {errorMsg}
          </div>
        )}
      </div>
    </div>
  );
}
