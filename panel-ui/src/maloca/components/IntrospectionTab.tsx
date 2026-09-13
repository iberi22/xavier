import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "motion/react";
import {
  Brain,
  ChevronRight,
  X,
  Send,
  Lightbulb,
  AlertTriangle,
  GitBranch,
  HelpCircle,
  Zap,
  CheckCircle2,
  Loader2,
  ArrowLeft,
} from "lucide-react";
import { getApiUrl } from "../../api/client";

// ─── Types ───────────────────────────────────────────────────────────────────

type ChallengeType = "contradiction" | "decision" | "execution" | "assumption" | "clarification";
type Technique =
  | "socratic_questioning"
  | "five_whys"
  | "pre_mortem"
  | "steel_manning"
  | "first_principles"
  | "pattern_recognition";
type SessionStatus = "active" | "completed" | "abandoned";

interface Challenge {
  id: string;
  session_id: string;
  challenge_type: ChallengeType;
  description: string;
  raw_content: string;
  confidence_score: number;
  status: string;
  recommended_technique: Technique;
}

interface Turn {
  role: "llm_guide" | "human";
  content: string;
  timestamp: string;
}

interface IntrospectionSession {
  session_id: string;
  challenge_id: string;
  technique: Technique;
  turn_number: number;
  llm_prompt: string;
  depth_score: number;
  status: SessionStatus;
  insights: string[];
  is_complete: boolean;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

const TECHNIQUE_LABELS: Record<Technique, string> = {
  socratic_questioning: "Método Socrático",
  five_whys: "5 Porqués",
  pre_mortem: "Pre-Mortem",
  steel_manning: "Steel Manning",
  first_principles: "Primeros Principios",
  pattern_recognition: "Reconocimiento de Patrones",
};

const TECHNIQUE_COLORS: Record<Technique, string> = {
  socratic_questioning: "text-violet-400 bg-violet-500/10 border-violet-500/30",
  five_whys: "text-amber-400 bg-amber-500/10 border-amber-500/30",
  pre_mortem: "text-red-400 bg-red-500/10 border-red-500/30",
  steel_manning: "text-blue-400 bg-blue-500/10 border-blue-500/30",
  first_principles: "text-emerald-400 bg-emerald-500/10 border-emerald-500/30",
  pattern_recognition: "text-cyan-400 bg-cyan-500/10 border-cyan-500/30",
};

const TYPE_ICON: Record<ChallengeType, React.ReactNode> = {
  contradiction: <GitBranch size={20} className="text-red-400" />,
  decision: <Zap size={20} className="text-blue-400" />,
  execution: <CheckCircle2 size={20} className="text-emerald-400" />,
  assumption: <HelpCircle size={20} className="text-amber-400" />,
  clarification: <Lightbulb size={20} className="text-violet-400" />,
};

const TYPE_BG: Record<ChallengeType, string> = {
  contradiction: "bg-red-500/10 border-red-500/20",
  decision: "bg-blue-500/10 border-blue-500/20",
  execution: "bg-emerald-500/10 border-emerald-500/20",
  assumption: "bg-amber-500/10 border-amber-500/20",
  clarification: "bg-violet-500/10 border-violet-500/20",
};

const TYPE_ACCENT: Record<ChallengeType, string> = {
  contradiction: "border-l-red-500",
  decision: "border-l-blue-500",
  execution: "border-l-emerald-500",
  assumption: "border-l-amber-500",
  clarification: "border-l-violet-500",
};

const RECOMMENDED_TECHNIQUE: Record<ChallengeType, Technique> = {
  contradiction: "steel_manning",
  decision: "pre_mortem",
  execution: "first_principles",
  assumption: "socratic_questioning",
  clarification: "pattern_recognition",
};

function DepthDots({ score }: { score: number }) {
  const filled = Math.round(score * 5);
  return (
    <div className="flex gap-1 items-center">
      {Array.from({ length: 5 }).map((_, i) => (
        <div
          key={i}
          className={`w-2 h-2 rounded-full transition-colors ${
            i < filled ? "bg-emerald-400" : "bg-white/15"
          }`}
        />
      ))}
    </div>
  );
}

// ─── Session Card ─────────────────────────────────────────────────────────────

function SessionCard({
  challenge,
  onEnter,
}: {
  challenge: Challenge;
  onEnter: (challenge: Challenge) => void;
}) {
  const technique = RECOMMENDED_TECHNIQUE[challenge.challenge_type];
  const title =
    challenge.description ||
    challenge.raw_content.slice(0, 80) + (challenge.raw_content.length > 80 ? "..." : "");
  const preview = challenge.raw_content.slice(0, 120) + (challenge.raw_content.length > 120 ? "..." : "");

  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      className={`relative group bg-[#0d0d12] border border-white/8 border-l-2 ${TYPE_ACCENT[challenge.challenge_type]} rounded-xl p-5 hover:border-white/15 transition-all duration-200 cursor-default`}
    >
      <div className="flex items-start gap-4">
        {/* Type icon */}
        <div className={`mt-0.5 p-2.5 rounded-lg border ${TYPE_BG[challenge.challenge_type]} shrink-0`}>
          {TYPE_ICON[challenge.challenge_type]}
        </div>

        {/* Content */}
        <div className="flex-1 min-w-0">
          <div className="flex items-start justify-between gap-3 mb-2">
            <h3 className="text-sm font-semibold text-white leading-snug">{title}</h3>
          </div>

          {/* Technique + depth */}
          <div className="flex items-center gap-3 mb-3">
            <span
              className={`text-xs font-mono px-2 py-0.5 rounded border ${
                TECHNIQUE_COLORS[technique]
              }`}
            >
              {TECHNIQUE_LABELS[technique]}
            </span>
            <DepthDots score={0} />
          </div>

          {/* Preview */}
          <p className="text-xs text-white/40 italic leading-relaxed mb-4">
            &ldquo;{preview}&rdquo;
          </p>

          {/* Footer */}
          <div className="flex items-center justify-between">
            <span className="text-xs font-mono text-white/25 capitalize">
              {challenge.challenge_type} · confianza {Math.round(challenge.confidence_score * 100)}%
            </span>
            <button
              type="button"
              onClick={() => onEnter(challenge)}
              className="flex items-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-400 border border-emerald-500/30 rounded-lg transition-all group-hover:border-emerald-500/50"
            >
              Entrar
              <ChevronRight size={14} />
            </button>
          </div>
        </div>
      </div>
    </motion.div>
  );
}

// ─── Introspection Chat ───────────────────────────────────────────────────────

function IntrospectionChat({
  challenge,
  onBack,
}: {
  challenge: Challenge;
  onBack: () => void;
}) {
  const technique = RECOMMENDED_TECHNIQUE[challenge.challenge_type];
  const [session, setSession] = useState<IntrospectionSession | null>(null);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  // Start session on mount
  useEffect(() => {
    startSession();
  }, []);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [turns]);

  async function callIntrospect(payload: Record<string, unknown>) {
    const res = await fetch(getApiUrl("/v1/maloca/challenges/introspect"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!res.ok) throw new Error(await res.text());
    return (await res.json()) as IntrospectionSession;
  }

  async function startSession() {
    setLoading(true);
    setError(null);
    try {
      const data = await callIntrospect({
        challenge_id: challenge.id,
        technique: technique,
      });
      setSession(data);
      setTurns([{ role: "llm_guide", content: data.llm_prompt, timestamp: new Date().toISOString() }]);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Error al iniciar la sesión");
    } finally {
      setLoading(false);
    }
  }

  async function sendTurn() {
    if (!input.trim() || !session || loading) return;
    const userMsg = input.trim();
    setInput("");
    setTurns((prev) => [
      ...prev,
      { role: "human", content: userMsg, timestamp: new Date().toISOString() },
    ]);
    setLoading(true);
    try {
      const data = await callIntrospect({
        challenge_id: challenge.id,
        session_id: session.session_id,
        human_input: userMsg,
      });
      setSession(data);
      setTurns((prev) => [
        ...prev,
        { role: "llm_guide", content: data.llm_prompt, timestamp: new Date().toISOString() },
      ]);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Error al procesar tu respuesta");
    } finally {
      setLoading(false);
    }
  }

  async function completeSession() {
    if (!session) return;
    setLoading(true);
    try {
      const data = await callIntrospect({
        challenge_id: challenge.id,
        session_id: session.session_id,
        complete: true,
      });
      setSession(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Error al completar la sesión");
    } finally {
      setLoading(false);
    }
  }

  const isComplete = session?.is_complete ?? false;

  return (
    <div className="flex flex-col h-full">
      {/* Chat header */}
      <div className="flex items-center gap-3 pb-4 border-b border-white/8 shrink-0">
        <button
          type="button"
          onClick={onBack}
          className="p-1.5 rounded-lg hover:bg-white/5 text-white/40 hover:text-white transition-colors"
        >
          <ArrowLeft size={18} />
        </button>
        <div className={`p-2 rounded-lg border ${TYPE_BG[challenge.challenge_type]}`}>
          {TYPE_ICON[challenge.challenge_type]}
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-semibold text-white truncate">
            {challenge.description || challenge.raw_content.slice(0, 60)}
          </p>
          <div className="flex items-center gap-2 mt-0.5">
            <span className={`text-xs font-mono px-1.5 py-0.5 rounded border ${TECHNIQUE_COLORS[technique]}`}>
              {TECHNIQUE_LABELS[technique]}
            </span>
            {session && <DepthDots score={session.depth_score} />}
          </div>
        </div>
        {session && !isComplete && (
          <button
            type="button"
            onClick={completeSession}
            className="text-xs px-3 py-1.5 rounded-lg bg-white/5 hover:bg-white/10 text-white/50 hover:text-white border border-white/10 transition-all"
          >
            Completar
          </button>
        )}
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto py-4 space-y-4 pr-1">
        <AnimatePresence initial={false}>
          {turns.map((turn, i) => (
            <motion.div
              key={i}
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.2 }}
              className={`flex ${
                turn.role === "human" ? "justify-end" : "justify-start"
              }`}
            >
              {turn.role === "llm_guide" && (
                <div className="flex items-start gap-2 max-w-[85%]">
                  <div className="mt-1 p-1.5 rounded-full bg-emerald-500/10 border border-emerald-500/20 shrink-0">
                    <Brain size={14} className="text-emerald-400" />
                  </div>
                  <div className="bg-[#0d0d12] border border-white/8 rounded-2xl rounded-tl-sm px-4 py-3">
                    <p className="text-sm text-white/85 leading-relaxed">{turn.content}</p>
                  </div>
                </div>
              )}
              {turn.role === "human" && (
                <div className="max-w-[85%] bg-emerald-500/10 border border-emerald-500/20 rounded-2xl rounded-tr-sm px-4 py-3">
                  <p className="text-sm text-emerald-100 leading-relaxed">{turn.content}</p>
                </div>
              )}
            </motion.div>
          ))}
        </AnimatePresence>

        {loading && (
          <div className="flex justify-start">
            <div className="flex items-center gap-2">
              <div className="p-1.5 rounded-full bg-emerald-500/10 border border-emerald-500/20">
                <Brain size={14} className="text-emerald-400" />
              </div>
              <div className="bg-[#0d0d12] border border-white/8 rounded-2xl px-4 py-3">
                <Loader2 size={16} className="text-white/40 animate-spin" />
              </div>
            </div>
          </div>
        )}

        {/* Completion insights */}
        {isComplete && session && session.insights.length > 0 && (
          <motion.div
            initial={{ opacity: 0, scale: 0.98 }}
            animate={{ opacity: 1, scale: 1 }}
            className="bg-emerald-500/5 border border-emerald-500/20 rounded-xl p-4"
          >
            <div className="flex items-center gap-2 mb-3">
              <Lightbulb size={16} className="text-emerald-400" />
              <span className="text-xs font-semibold text-emerald-400 uppercase tracking-wider">
                Insights extraídos
              </span>
            </div>
            <ul className="space-y-2">
              {session.insights.map((insight, i) => (
                <li key={i} className="flex items-start gap-2">
                  <span className="text-emerald-400/50 text-xs mt-0.5 font-mono">{i + 1}.</span>
                  <p className="text-xs text-white/70 leading-relaxed">{insight}</p>
                </li>
              ))}
            </ul>
          </motion.div>
        )}

        {error && (
          <div className="flex items-center gap-2 text-xs text-red-400 bg-red-500/5 border border-red-500/20 rounded-lg px-3 py-2">
            <AlertTriangle size={14} />
            {error}
          </div>
        )}

        <div ref={bottomRef} />
      </div>

      {/* Input */}
      {!isComplete ? (
        <div className="pt-3 border-t border-white/8 shrink-0">
          <div className="flex gap-2">
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  sendTurn();
                }
              }}
              placeholder="Escribe tu respuesta... (Enter para enviar, Shift+Enter para nueva línea)"
              className="flex-1 bg-[#0d0d12] border border-white/10 rounded-xl px-4 py-3 text-sm text-white placeholder:text-white/25 resize-none focus:outline-none focus:border-emerald-500/40 transition-colors"
              rows={2}
              disabled={loading}
            />
            <button
              type="button"
              onClick={sendTurn}
              disabled={!input.trim() || loading}
              className="self-end p-3 rounded-xl bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-400 border border-emerald-500/30 disabled:opacity-30 disabled:cursor-not-allowed transition-all"
            >
              <Send size={16} />
            </button>
          </div>
        </div>
      ) : (
        <div className="pt-3 border-t border-white/8 shrink-0">
          <div className="flex items-center justify-center gap-2 text-xs text-emerald-400/70 font-mono">
            <CheckCircle2 size={14} />
            Sesión completada · Los insights serán marcados para entrenamiento
          </div>
        </div>
      )}
    </div>
  );
}

// ─── Main Tab ─────────────────────────────────────────────────────────────────

export function IntrospectionTab() {
  const [challenges, setChallenges] = useState<Challenge[]>([]);
  const [loading, setLoading] = useState(false);
  const [activeChallenge, setActiveChallenge] = useState<Challenge | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchAvailable();
  }, []);

  async function fetchAvailable() {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(getApiUrl("/v1/maloca/introspection/available"));
      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();
      const raw = (data.challenges ?? []) as Challenge[];
      // Add recommended technique
      setChallenges(
        raw.map((c) => ({
          ...c,
          recommended_technique: RECOMMENDED_TECHNIQUE[c.challenge_type] ?? "socratic_questioning",
        }))
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : "Error al cargar sesiones");
    } finally {
      setLoading(false);
    }
  }

  if (activeChallenge) {
    return (
      <IntrospectionChat
        challenge={activeChallenge}
        onBack={() => setActiveChallenge(null)}
      />
    );
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between mb-6 shrink-0">
        <div className="flex items-center gap-3">
          <div className="p-2 rounded-xl bg-violet-500/10 border border-violet-500/20">
            <Brain size={20} className="text-violet-400" />
          </div>
          <div>
            <h2 className="text-base font-semibold text-white">Sesiones de Introspección</h2>
            <p className="text-xs text-white/40 font-mono">
              Xavier detectó momentos para reflexión profunda
            </p>
          </div>
          {challenges.length > 0 && (
            <span className="ml-1 px-2 py-0.5 rounded-full bg-violet-500/20 text-violet-300 text-xs font-bold">
              {challenges.length}
            </span>
          )}
        </div>
        <button
          type="button"
          onClick={fetchAvailable}
          disabled={loading}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-mono bg-white/5 hover:bg-white/8 text-white/50 hover:text-white border border-white/10 rounded-lg transition-all disabled:opacity-30"
        >
          {loading ? <Loader2 size={13} className="animate-spin" /> : "↻"} Actualizar
        </button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto pr-1">
        {loading && challenges.length === 0 && (
          <div className="flex items-center justify-center h-48">
            <Loader2 size={24} className="text-white/20 animate-spin" />
          </div>
        )}

        {error && (
          <div className="flex items-center gap-2 text-xs text-red-400 bg-red-500/5 border border-red-500/20 rounded-xl px-4 py-3 mb-4">
            <AlertTriangle size={14} />
            {error}
          </div>
        )}

        {!loading && challenges.length === 0 && !error && (
          <div className="flex flex-col items-center justify-center h-48 text-center">
            <Brain size={36} className="text-white/10 mb-3" />
            <p className="text-sm text-white/30 font-mono">
              No hay sesiones disponibles aún
            </p>
            <p className="text-xs text-white/20 mt-1">
              Xavier generará sesiones a medida que detecte patrones en tus conversaciones
            </p>
          </div>
        )}

        <div className="space-y-3">
          {challenges.map((challenge) => (
            <SessionCard
              key={challenge.id}
              challenge={challenge}
              onEnter={setActiveChallenge}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
