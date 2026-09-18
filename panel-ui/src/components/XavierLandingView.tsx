import React, { useState } from "react";
import {
  Shield,
  Terminal,
  Database,
  ExternalLink,
  Sparkles,
  Lock,
  Boxes,
  ArrowRight,
} from "lucide-react";

export function XavierLandingView() {
  const [activePrivacy, setActivePrivacy] = useState<"p4" | "p3">("p4");

  const CLOUD_URL = "https://xavier-cloud.swal.network";

  const pipelineSteps = [
    {
      phase: "01",
      title: "Cognitive Ingestion",
      tech: "sqlite-vec + AST Multi-Language",
      desc: "Sub-millisecond vector indexing, symbol call hierarchies, and execution traces stored strictly on-device.",
    },
    {
      phase: "02",
      title: "Socratic Introspection",
      tech: "IntrospectionEngine + CurationGate",
      desc: "Distill root causes, challenge architectural assumptions, and produce verified ground-truth training pairs.",
    },
    {
      phase: "03",
      title: "Differential Privacy",
      tech: "P4 Local / P3 Zero-Leak",
      desc: "Automated cryptographic scrubbing of PII, secrets, keys, and file paths with Laplacian perturbation.",
    },
    {
      phase: "04",
      title: "Micro-Experts SLM",
      tech: "GGUF / Q4_K_M (0.5B – 3B)",
      desc: "Specialized models (AST blast-radius, MCP JSON-RPC caller) running at 120+ tokens/s with 0 API cost.",
    },
  ];

  const microExperts = [
    {
      name: "AST Blast-Radius Expert",
      base: "Qwen2.5-Coder-0.5B",
      size: "380 MB",
      task: "Predicts code refactor impacts and deep call chains with zero latency.",
      speed: "140 t/s",
    },
    {
      name: "MCP Tool-Calling Runner",
      base: "SmolLM2-1.7B",
      size: "980 MB",
      task: "High-precision JSON-RPC tool orchestration and parameter synthesis.",
      speed: "95 t/s",
    },
    {
      name: "Socratic Ground-Truth Critic",
      base: "Llama-3.2-1B",
      size: "620 MB",
      task: "5-Whys root-cause verification and contradiction detection in memory.",
      speed: "115 t/s",
    },
  ];

  return (
    <div className="min-h-screen bg-[#050505] text-slate-100 font-sans selection:bg-emerald-500/30 selection:text-emerald-200 relative overflow-x-hidden">
      {/* Background Gradients */}
      <div className="fixed inset-0 pointer-events-none z-0">
        <div className="absolute -top-40 left-1/2 -translate-x-1/2 w-[1000px] h-[500px] bg-emerald-500/10 rounded-full blur-[140px]" />
        <div className="absolute top-[600px] -left-40 w-[600px] h-[600px] bg-indigo-500/10 rounded-full blur-[160px]" />
        <div className="absolute top-[1200px] -right-40 w-[700px] h-[700px] bg-cyan-500/10 rounded-full blur-[160px]" />
        <div className="absolute inset-0 bg-[radial-gradient(ellipse_80%_80%_at_50%_-20%,rgba(16,185,129,0.15),rgba(255,255,255,0))]" />
      </div>

      {/* Top Navbar */}
      <nav className="relative z-20 border-b border-white/10 bg-black/40 backdrop-blur-xl sticky top-0">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-16 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-emerald-500/20 border border-emerald-500/40 flex items-center justify-center text-emerald-400 font-mono font-bold text-sm shadow-[0_0_15px_rgba(16,185,129,0.3)]">
              X
            </div>
            <div>
              <span className="font-bold tracking-wider text-white text-lg font-mono">XAVIER</span>
              <span className="hidden sm:inline-block ml-2 text-[10px] uppercase font-mono px-2 py-0.5 rounded bg-emerald-500/10 border border-emerald-500/30 text-emerald-400">
                Cognitive Core
              </span>
            </div>
          </div>

          <div className="flex items-center gap-3">
            <a
              href="https://swal.network"
              className="text-xs font-mono text-slate-400 hover:text-white px-3 py-1.5 rounded-lg hover:bg-white/5 transition-all hidden md:flex items-center gap-1.5"
            >
              SWAL Sovereign Mesh ↗
            </a>
            <a
              href="https://github.com/iberi22/xavier"
              target="_blank"
              rel="noopener noreferrer"
              className="text-xs font-mono text-slate-400 hover:text-white px-3 py-1.5 rounded-lg hover:bg-white/5 transition-all hidden sm:inline-block"
            >
              GitHub (v0.25)
            </a>
            <a
              href={CLOUD_URL}
              className="inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-emerald-500 hover:bg-emerald-400 text-black text-xs font-mono font-bold transition-all shadow-[0_0_20px_rgba(16,185,129,0.4)] hover:scale-[1.02] active:scale-[0.98]"
            >
              <span>Acceder a Xavier Cloud</span>
              <ArrowRight className="w-3.5 h-3.5" />
            </a>
          </div>
        </div>
      </nav>

      {/* Hero Section */}
      <section className="relative z-10 pt-20 pb-24 px-4 sm:px-6 lg:px-8 max-w-7xl mx-auto text-center">
        <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-emerald-500/10 border border-emerald-500/30 text-emerald-400 text-xs font-mono mb-8 animate-pulse">
          <Sparkles className="w-3.5 h-3.5" />
          <span>NATIVO EN RUST 2024 · VECTOR RUNTIME & ON-DEMAND SLMs</span>
        </div>

        <h1 className="text-4xl sm:text-6xl lg:text-7xl font-extrabold tracking-tight text-white mb-6 leading-tight">
          La memoria cognitiva <br />
          <span className="bg-clip-text text-transparent bg-gradient-to-r from-emerald-400 via-cyan-400 to-indigo-400 drop-shadow-[0_0_35px_rgba(16,185,129,0.3)]">
            para agentes soberanos
          </span>
        </h1>

        <p className="max-w-3xl mx-auto text-base sm:text-lg text-slate-400 leading-relaxed mb-10">
          Xavier es un runtime de memoria vectorial persistente y grafo de código para agentes de IA.
          Indexa bases de código complejas, destila heurísticas mediante introspección socrática y
          permite sintetizar mini-expertos de 0.5B a 3B sin fugas de privacidad.
        </p>

        {/* CTA Group */}
        <div className="flex flex-col sm:flex-row items-center justify-center gap-4 mb-16">
          <a
            href={CLOUD_URL}
            className="w-full sm:w-auto inline-flex items-center justify-center gap-2 px-6 py-3.5 rounded-xl bg-emerald-500 hover:bg-emerald-400 text-black text-sm font-mono font-bold transition-all shadow-[0_0_25px_rgba(16,185,129,0.5)] hover:scale-[1.03] active:scale-[0.98]"
          >
            <span>Lanzar Xavier Cloud Platform</span>
            <ExternalLink className="w-4 h-4" />
          </a>
          <a
            href="https://github.com/iberi22/xavier/releases"
            target="_blank"
            rel="noopener noreferrer"
            className="w-full sm:w-auto inline-flex items-center justify-center gap-2 px-6 py-3.5 rounded-xl bg-white/5 hover:bg-white/10 border border-white/10 text-white text-sm font-mono transition-all hover:border-white/20"
          >
            <Terminal className="w-4 h-4 text-emerald-400" />
            <span>Descargar Binario Daemon (Linux/macOS)</span>
          </a>
        </div>

        {/* Live Metrics Grid */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 max-w-4xl mx-auto">
          <div className="p-4 rounded-xl bg-white/[0.03] border border-white/10 backdrop-blur-md">
            <div className="text-2xl font-bold font-mono text-emerald-400">&lt; 85ms</div>
            <div className="text-xs text-slate-400 mt-1 font-mono uppercase">Arranque Daemon</div>
          </div>
          <div className="p-4 rounded-xl bg-white/[0.03] border border-white/10 backdrop-blur-md">
            <div className="text-2xl font-bold font-mono text-cyan-400">&lt; 1ms</div>
            <div className="text-xs text-slate-400 mt-1 font-mono uppercase">sqlite-vec Search</div>
          </div>
          <div className="p-4 rounded-xl bg-white/[0.03] border border-white/10 backdrop-blur-md">
            <div className="text-2xl font-bold font-mono text-indigo-400">120+ t/s</div>
            <div className="text-xs text-slate-400 mt-1 font-mono uppercase">Inferencia Local</div>
          </div>
          <div className="p-4 rounded-xl bg-white/[0.03] border border-white/10 backdrop-blur-md">
            <div className="text-2xl font-bold font-mono text-white">0 Leaks</div>
            <div className="text-xs text-slate-400 mt-1 font-mono uppercase">P4 Cero Telemetría</div>
          </div>
        </div>
      </section>

      {/* Pipeline Section */}
      <section className="relative z-10 py-20 px-4 sm:px-6 lg:px-8 max-w-7xl mx-auto border-t border-white/10">
        <div className="text-center mb-16">
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-cyan-500/10 border border-cyan-500/30 text-cyan-400 text-xs font-mono mb-3">
            END-TO-END REASONING PIPELINE
          </div>
          <h2 className="text-3xl sm:text-4xl font-bold text-white mb-4">
            De fragmentos de memoria a modelos privados on-demand
          </h2>
          <p className="text-slate-400 text-sm max-w-2xl mx-auto">
            El flujo soberano de Xavier permite cerrar el bucle de razonamiento de agentes autónomos
            sin depender de infraestructura centralizada ni APIs de terceros.
          </p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
          {pipelineSteps.map((step) => (
            <div
              key={step.phase}
              className="p-6 rounded-2xl bg-white/[0.02] border border-white/10 hover:border-emerald-500/40 transition-all duration-300 group flex flex-col justify-between hover:bg-white/[0.04]"
            >
              <div>
                <span className="text-xs font-mono font-bold text-emerald-400">FASE {step.phase}</span>
                <h3 className="text-lg font-bold text-white mt-2 mb-3 group-hover:text-emerald-300 transition-colors">
                  {step.title}
                </h3>
                <p className="text-xs text-slate-400 leading-relaxed mb-4">
                  {step.desc}
                </p>
              </div>
              <div className="pt-3 border-t border-white/5 text-[10px] font-mono text-slate-500">
                {step.tech}
              </div>
            </div>
          ))}
        </div>
      </section>

      {/* Privacy Protocols Section */}
      <section className="relative z-10 py-20 px-4 sm:px-6 lg:px-8 max-w-7xl mx-auto border-t border-white/10">
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 items-center">
          <div className="lg:col-span-6 space-y-6">
            <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-indigo-500/10 border border-indigo-500/30 text-indigo-400 text-xs font-mono">
              <Shield className="w-3.5 h-3.5" />
              <span>PRIVACIDAD DIFERENCIAL Y ED25519</span>
            </div>
            <h2 className="text-3xl sm:text-4xl font-bold text-white">
              Tus secretos, heurísticas y código nunca abandonan tu máquina
            </h2>
            <p className="text-slate-400 text-sm leading-relaxed">
              Xavier implementa identidad soberana basada en Ed25519 (BIP39-24) y dos niveles estrictos
              de exportación de datos. Ya sea en entrenamiento 100% on-device o en clústeres de Colab,
              el protocolo previene la filtración de tokens, rutas o variables de entorno.
            </p>

            <div className="flex gap-2 p-1.5 bg-white/5 border border-white/10 rounded-xl w-fit font-mono text-xs">
              <button
                type="button"
                onClick={() => setActivePrivacy("p4")}
                className={`px-4 py-2 rounded-lg transition-all ${
                  activePrivacy === "p4"
                    ? "bg-emerald-500 text-black font-bold shadow-[0_0_15px_rgba(16,185,129,0.3)]"
                    : "text-slate-400 hover:text-white"
                }`}
              >
                P4: Local Solo
              </button>
              <button
                type="button"
                onClick={() => setActivePrivacy("p3")}
                className={`px-4 py-2 rounded-lg transition-all ${
                  activePrivacy === "p3"
                    ? "bg-indigo-500 text-white font-bold shadow-[0_0_15px_rgba(99,102,241,0.3)]"
                    : "text-slate-400 hover:text-white"
                }`}
              >
                P3: Colab Anonimizado
              </button>
            </div>

            <div className="p-5 rounded-xl bg-white/[0.02] border border-white/10 space-y-3 font-mono text-xs">
              {activePrivacy === "p4" ? (
                <>
                  <div className="flex items-center justify-between text-emerald-400 font-bold">
                    <span>Nivel P4: 100% On-Device</span>
                    <Lock className="w-4 h-4" />
                  </div>
                  <p className="text-slate-400 text-[11px] font-sans">
                    Cero conexiones externas. Los pesos GGUF se ejecutan con soporte para AVX-512,
                    Metal (M1-M4) o CUDA local. Tus datos jamás viajan por la red.
                  </p>
                </>
              ) : (
                <>
                  <div className="flex items-center justify-between text-indigo-400 font-bold">
                    <span>Nivel P3: Zero-Leak Dataset</span>
                    <Shield className="w-4 h-4" />
                  </div>
                  <p className="text-slate-400 text-[11px] font-sans">
                    Scrubbing algorítmico exhaustivo. Reemplaza variables de entorno y tokens con
                    identificadores k-anonymity y ruido Laplaciano para usar GPUs libres en Colab sin riesgo.
                  </p>
                </>
              )}
            </div>
          </div>

          <div className="lg:col-span-6 space-y-4">
            <h3 className="text-xs font-mono uppercase tracking-wider text-slate-400 flex items-center gap-2">
              <Boxes className="w-4 h-4 text-cyan-400" />
              Catálogo de Micro-Expertos Sintetizables
            </h3>
            {microExperts.map((exp) => (
              <div
                key={exp.name}
                className="p-5 rounded-xl bg-white/[0.02] border border-white/10 hover:border-white/20 transition-all"
              >
                <div className="flex items-center justify-between mb-2">
                  <span className="font-mono font-bold text-white text-sm">{exp.name}</span>
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-400 border border-emerald-500/20">
                    {exp.speed}
                  </span>
                </div>
                <div className="text-xs text-slate-400 mb-3">{exp.task}</div>
                <div className="flex items-center gap-4 text-[10px] font-mono text-slate-500">
                  <span>Base: {exp.base}</span>
                  <span>Tamaño: {exp.size}</span>
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Cloud & Mesh Section */}
      <section className="relative z-10 py-20 px-4 sm:px-6 lg:px-8 max-w-7xl mx-auto border-t border-white/10 text-center">
        <div className="max-w-3xl mx-auto p-10 rounded-3xl bg-gradient-to-b from-emerald-500/10 to-black/60 border border-emerald-500/30 backdrop-blur-xl relative overflow-hidden">
          <div className="absolute top-0 right-0 p-8 opacity-10 pointer-events-none">
            <Database className="w-48 h-48 text-emerald-400" />
          </div>

          <h2 className="text-2xl sm:text-3xl font-bold text-white mb-4">
            ¿Listo para orquestar tu nodo en la nube?
          </h2>
          <p className="text-sm text-slate-400 mb-8 max-w-xl mx-auto">
            Inicia sesión en Xavier Cloud (<code className="text-emerald-300">xavier-cloud.swal.network</code>)
            para sincronizar chunks cifrados vía Shamir 2-de-3 y monitorear la salud de tus agentes.
          </p>

          <div className="flex flex-col sm:flex-row items-center justify-center gap-4">
            <a
              href={CLOUD_URL}
              className="w-full sm:w-auto inline-flex items-center justify-center gap-2 px-8 py-3.5 rounded-xl bg-emerald-500 hover:bg-emerald-400 text-black text-sm font-mono font-bold transition-all shadow-[0_0_30px_rgba(16,185,129,0.5)]"
            >
              <span>Abrir Xavier Cloud</span>
              <ArrowRight className="w-4 h-4" />
            </a>
            <a
              href="https://swal.network"
              className="w-full sm:w-auto inline-flex items-center justify-center gap-2 px-6 py-3.5 rounded-xl bg-white/5 hover:bg-white/10 border border-white/10 text-slate-300 text-sm font-mono transition-all"
            >
              <span>Conocer la Red SWAL</span>
              <ExternalLink className="w-4 h-4" />
            </a>
          </div>
        </div>
      </section>

      {/* Footer */}
      <footer className="relative z-10 border-t border-white/10 py-8 px-4 sm:px-6 lg:px-8 max-w-7xl mx-auto flex flex-col sm:flex-row items-center justify-between gap-4 text-xs font-mono text-slate-500">
        <div>
          Xavier Memory Runtime · Parte del Ecosistema Soberano SWAL
        </div>
        <div className="flex items-center gap-6">
          <a href={CLOUD_URL} className="hover:text-emerald-400 transition-colors">
            Xavier Cloud
          </a>
          <a href="https://swal.network" className="hover:text-cyan-400 transition-colors">
            swal.network
          </a>
          <a href="https://github.com/iberi22/xavier" className="hover:text-white transition-colors">
            GitHub
          </a>
        </div>
      </footer>
    </div>
  );
}
