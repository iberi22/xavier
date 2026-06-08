import { Cpu, Monitor, Zap, Server } from 'lucide-react';
import { SystemInfo } from './OnboardingFlow';

export function HardwareStep({
  systemInfo,
  useGpu,
  onChangeUseGpu,
  onNext,
}: {
  systemInfo: SystemInfo;
  useGpu: boolean;
  onChangeUseGpu: (val: boolean) => void;
  onNext: () => void;
}) {
  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="text-center space-y-2">
        <h2 className="text-2xl font-bold text-white">NEURAL_EXECUTION_PLAN</h2>
        <p className="text-emerald-500/70 text-sm">Hardware allocation for LLM inference</p>
      </div>

      <div className="grid grid-cols-2 gap-4">
        {/* GPU Option */}
        <div
          onClick={() => onChangeUseGpu(true)}
          className={`p-4 rounded border cursor-pointer transition-all duration-300 ${
            useGpu
              ? 'bg-emerald-950/40 border-emerald-500 shadow-[0_0_15px_rgba(16,185,129,0.2)]'
              : 'bg-black border-neutral-800 hover:border-emerald-900'
          } ${!systemInfo.has_gpu && 'opacity-50'}`}
        >
          <div className="flex justify-between items-start mb-2">
            <Monitor className={`w-6 h-6 ${useGpu ? 'text-emerald-400' : 'text-neutral-500'}`} />
            {systemInfo.has_gpu && <Zap className="w-4 h-4 text-amber-400" />}
          </div>
          <h3 className={`font-bold ${useGpu ? 'text-white' : 'text-neutral-300'}`}>GPU Accleration</h3>
          <p className="text-xs text-neutral-500 mt-1">Recommended for fast inference.</p>
          {!systemInfo.has_gpu && <p className="text-xs text-red-400 mt-2">No supported GPU detected</p>}
        </div>

        {/* CPU Option */}
        <div
          onClick={() => onChangeUseGpu(false)}
          className={`p-4 rounded border cursor-pointer transition-all duration-300 ${
            !useGpu
              ? 'bg-emerald-950/40 border-emerald-500 shadow-[0_0_15px_rgba(16,185,129,0.2)]'
              : 'bg-black border-neutral-800 hover:border-emerald-900'
          }`}
        >
          <div className="flex justify-between items-start mb-2">
            <Cpu className={`w-6 h-6 ${!useGpu ? 'text-emerald-400' : 'text-neutral-500'}`} />
            <Server className="w-4 h-4 text-emerald-600" />
          </div>
          <h3 className={`font-bold ${!useGpu ? 'text-white' : 'text-neutral-300'}`}>CPU Fallback</h3>
          <p className="text-xs text-neutral-500 mt-1">Slower, uses System RAM ({systemInfo.total_ram_gb.toFixed(0)} GB).</p>
        </div>
      </div>

      <div className="bg-neutral-950 p-4 border border-emerald-900/30 rounded text-sm text-emerald-300/80">
        <p>
          Xavier will configure the internal settings to use <strong>{useGpu ? 'gpu-fast-model' : 'cpu-fast-model'}</strong>. You can change this later in settings.
        </p>
      </div>

      <div className="flex justify-end pt-4">
        <button
          onClick={onNext}
          className="px-6 py-2 bg-emerald-600 hover:bg-emerald-500 text-black font-bold rounded transition-colors"
        >
          CONFIRM_ALLOCATION
        </button>
      </div>
    </div>
  );
}
