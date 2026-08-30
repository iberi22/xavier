import React, { useState, useMemo, useCallback } from "react";
import {
  Vote,
  CheckCircle2,
  XCircle,
  MinusCircle,
  Clock,
  ShieldAlert,
  ShieldCheck,
  Search,
  Filter,
  RefreshCw,
  Award,
  User,
  Check,
  Lock,
  BarChart3,
} from "lucide-react";

export type VoteChoice = "for" | "against" | "abstain";
export type ProposalStatus = "active" | "passed" | "rejected" | "expired" | "pending";

export interface ProposalVotes {
  for: number;
  against: number;
  abstain: number;
}

export interface ProposalQuorum {
  current: number;
  required: number;
}

export interface Proposal {
  id: string;
  title: string;
  description: string;
  status: ProposalStatus;
  authorNode: string;
  requiredEndorsement: string;
  expiresAt: string | number;
  votes: ProposalVotes;
  quorum: ProposalQuorum;
  userVote?: VoteChoice | null;
}

export interface DaoGovernancePanelProps {
  proposals?: Proposal[];
  currentNodeId?: string;
  currentNodeEndorsements?: string[];
  onVote?: (proposalId: string, choice: VoteChoice) => Promise<void> | void;
  onRefresh?: () => void;
  isLoading?: boolean;
  error?: string | null;
}

// Default mock proposals for DAO governance panel
const DEFAULT_PROPOSALS: Proposal[] = [
  {
    id: "prop-001",
    title: "XW-16 Mesh Protocol Upgrade & Parameter Tuning",
    description:
      "Upgrade P2P transport backoff limits and lower baseline heartbeat interval from 15s to 10s across validator peers.",
    status: "active",
    authorNode: "node-validator-01",
    requiredEndorsement: "validator",
    expiresAt: new Date(Date.now() + 86400000 * 2.5).toISOString(),
    votes: { for: 42, against: 8, abstain: 4 },
    quorum: { current: 54, required: 60 },
    userVote: null,
  },
  {
    id: "prop-002",
    title: "Telemetry Redaction Rule Set Revision v2",
    description:
      "Extend telemetry sanitizer redaction patterns to strip internal workspace IPv6 subnets and developer machine hostnames.",
    status: "active",
    authorNode: "node-security-09",
    requiredEndorsement: "security-lead",
    expiresAt: new Date(Date.now() + 86400000 * 5).toISOString(),
    votes: { for: 18, against: 2, abstain: 1 },
    quorum: { current: 21, required: 50 },
    userVote: null,
  },
  {
    id: "prop-003",
    title: "Allocation of Vector Embedding Index Cache Budget",
    description:
      "Expand memory store mmap cache buffer threshold to 512MB for high-throughput CodeGraph DB queries.",
    status: "passed",
    authorNode: "node-core-03",
    requiredEndorsement: "core",
    expiresAt: new Date(Date.now() - 86400000 * 1.2).toISOString(),
    votes: { for: 85, against: 12, abstain: 3 },
    quorum: { current: 100, required: 75 },
    userVote: "for",
  },
  {
    id: "prop-004",
    title: "Deprecate Legacy v1 HTTP Training Route Endpoint",
    description:
      "Mark legacy synchronous training dataset generation routes as deprecated in favor of streamed jsonl payloads.",
    status: "rejected",
    authorNode: "node-dev-05",
    requiredEndorsement: "validator",
    expiresAt: new Date(Date.now() - 86400000 * 4).toISOString(),
    votes: { for: 14, against: 62, abstain: 4 },
    quorum: { current: 80, required: 60 },
    userVote: "against",
  },
];

/**
 * Helper to calculate time remaining string from ISO string or timestamp
 */
function formatTimeRemaining(expiresAt: string | number): string {
  const expTime = typeof expiresAt === "string" ? new Date(expiresAt).getTime() : expiresAt;
  const diff = expTime - Date.now();
  if (diff <= 0) return "Expired";
  const hours = Math.floor(diff / (1000 * 60 * 60));
  const days = Math.floor(hours / 24);
  const remHours = hours % 24;
  if (days > 0) {
    return `${days}d ${remHours}h remaining`;
  }
  const minutes = Math.floor((diff % (1000 * 60 * 60)) / (1000 * 60));
  return `${hours}h ${minutes}m remaining`;
}

/**
 * Helper to check if node possesses the required endorsement
 */
function checkEndorsement(
  required: string,
  userEndorsements: string[]
): boolean {
  if (!required || required.trim() === "" || required === "none") {
    return true;
  }
  const normalizedRequired = required.toLowerCase().trim();
  return userEndorsements.some((e) => {
    const norm = e.toLowerCase().trim();
    return norm === "admin" || norm === "all" || norm === normalizedRequired;
  });
}

export function DaoGovernancePanel({
  proposals: propsProposals,
  currentNodeId = "node-local-01",
  currentNodeEndorsements = ["validator", "core"],
  onVote,
  onRefresh,
  isLoading = false,
  error = null,
}: DaoGovernancePanelProps) {
  // Use controlled proposals if passed, otherwise manage local mock state
  const [localProposals, setLocalProposals] = useState<Proposal[]>(
    propsProposals || DEFAULT_PROPOSALS
  );
  const [searchQuery, setSearchQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<string>("all");
  const [submittingVoteId, setSubmittingVoteId] = useState<string | null>(null);

  // Sync internal proposals if prop updates
  const activeProposals = propsProposals || localProposals;

  const handleVote = useCallback(
    async (proposalId: string, choice: VoteChoice) => {
      setSubmittingVoteId(proposalId);
      try {
        if (onVote) {
          await onVote(proposalId, choice);
        }
        // Update local state optimistic voting counts
        setLocalProposals((prev) =>
          prev.map((p) => {
            if (p.id !== proposalId) return p;
            const prevVote = p.userVote;
            const newVotes = { ...p.votes };

            // Adjust previous vote count if changing choice
            if (prevVote) {
              newVotes[prevVote] = Math.max(0, newVotes[prevVote] - 1);
            } else {
              // Increment quorum current if first time voting
              p.quorum = { ...p.quorum, current: p.quorum.current + 1 };
            }
            newVotes[choice] = newVotes[choice] + 1;

            return {
              ...p,
              userVote: choice,
              votes: newVotes,
            };
          })
        );
      } catch (err) {
        console.error("Failed to submit vote:", err);
      } finally {
        setSubmittingVoteId(null);
      }
    },
    [onVote]
  );

  const filteredProposals = useMemo(() => {
    return activeProposals.filter((p) => {
      const matchesSearch =
        p.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
        p.description.toLowerCase().includes(searchQuery.toLowerCase()) ||
        p.authorNode.toLowerCase().includes(searchQuery.toLowerCase()) ||
        p.id.toLowerCase().includes(searchQuery.toLowerCase());
      const matchesFilter =
        statusFilter === "all" || p.status.toLowerCase() === statusFilter.toLowerCase();
      return matchesSearch && matchesFilter;
    });
  }, [activeProposals, searchQuery, statusFilter]);

  return (
    <div className="space-y-6 p-6 overflow-y-auto h-full text-white/90">
      {/* Panel Header */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-[#39ff14]/10 border border-[#39ff14]/20 flex items-center justify-center text-[#39ff14]">
              <Vote className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-2xl font-light tracking-tight text-white">
                DAO Governance & Voting Center
              </h2>
              <p className="text-xs text-white/40 mt-0.5">
                Review mesh governance proposals, verify quorum parameters, and submit ballot actions.
              </p>
            </div>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <div className="text-right hidden sm:block">
            <p className="text-[10px] uppercase text-white/30 tracking-widest">
              Active Node
            </p>
            <code className="text-xs text-[#39ff14] font-mono select-all">
              {currentNodeId}
            </code>
          </div>

          {onRefresh && (
            <button
              type="button"
              onClick={onRefresh}
              disabled={isLoading}
              aria-label="Refresh proposals"
              className="px-3 py-2 bg-white/[0.03] border border-white/10 hover:border-white/20 rounded-xl text-xs text-white/80 transition-all flex items-center gap-1.5 disabled:opacity-50"
            >
              <RefreshCw className={`w-3.5 h-3.5 ${isLoading ? "animate-spin" : ""}`} />
              Refresh
            </button>
          )}
        </div>
      </div>

      {/* Error alert banner */}
      {error && (
        <div className="flex items-center gap-2 p-3 bg-red-500/10 border border-red-500/20 rounded-xl text-red-400 text-xs">
          <ShieldAlert className="w-4 h-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {/* Filter and Search Bar */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
        <div className="relative sm:col-span-2">
          <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-white/30" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search proposals by title, description, or author node..."
            className="w-full bg-black/40 border border-white/10 text-white/80 text-xs pl-9 pr-3 py-2.5 rounded-xl outline-none focus:border-[#39ff14]/40 transition-all font-mono placeholder:text-white/25"
          />
        </div>

        <div className="relative flex items-center">
          <Filter className="w-3.5 h-3.5 absolute left-3 text-white/30" />
          <select
            value={statusFilter}
            onChange={(e) => setStatusFilter(e.target.value)}
            className="w-full bg-black/40 border border-white/10 text-white/80 text-xs pl-9 pr-3 py-2.5 rounded-xl outline-none focus:border-[#39ff14]/40 transition-all font-mono appearance-none"
          >
            <option value="all">All Statuses</option>
            <option value="active">Active Proposals</option>
            <option value="passed">Passed Proposals</option>
            <option value="rejected">Rejected Proposals</option>
            <option value="expired">Expired Proposals</option>
          </select>
        </div>
      </div>

      {/* Proposals List */}
      <div className="space-y-4">
        {filteredProposals.length === 0 && (
          <div className="text-center py-12 bg-white/[0.01] border border-dashed border-white/5 rounded-2xl">
            <Vote className="w-8 h-8 text-white/20 mx-auto mb-2" />
            <p className="text-sm text-white/40">No DAO proposals match the selected filters.</p>
          </div>
        )}

        {filteredProposals.map((proposal) => {
          const hasEndorsement = checkEndorsement(
            proposal.requiredEndorsement,
            currentNodeEndorsements
          );

          // Quorum calculation
          const quorumRatio = proposal.quorum.required > 0
            ? proposal.quorum.current / proposal.quorum.required
            : 0;
          const quorumPercent = Math.min(100, Math.round(quorumRatio * 100));

          // Approval calculation
          const totalVotes = proposal.votes.for + proposal.votes.against + proposal.votes.abstain;
          const approvalPercent = totalVotes > 0
            ? Math.round((proposal.votes.for / totalVotes) * 100)
            : 0;

          const isSubmitting = submittingVoteId === proposal.id;
          const isVotingDisabled = !hasEndorsement || proposal.status !== "active" || isSubmitting;

          return (
            <div
              key={proposal.id}
              className="p-5 rounded-2xl bg-white/[0.02] border border-white/[0.06] hover:border-white/12 transition-all space-y-4"
            >
              {/* Proposal Header */}
              <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-3">
                <div className="space-y-1">
                  <div className="flex items-center gap-2.5 flex-wrap">
                    <StatusBadge status={proposal.status} />
                    <code className="text-[10px] text-white/30 font-mono bg-white/5 px-2 py-0.5 rounded">
                      {proposal.id}
                    </code>
                  </div>
                  <h3 className="text-base font-medium text-white/90 pt-1">
                    {proposal.title}
                  </h3>
                </div>

                {/* Active user vote tag */}
                {proposal.userVote && (
                  <div className="shrink-0 flex items-center gap-1.5 px-2.5 py-1 bg-white/5 border border-white/10 rounded-lg text-[11px] font-mono">
                    <span className="text-white/40">Your Vote:</span>
                    <span
                      className={
                        proposal.userVote === "for"
                          ? "text-[#39ff14] font-semibold uppercase"
                          : proposal.userVote === "against"
                          ? "text-red-400 font-semibold uppercase"
                          : "text-amber-400 font-semibold uppercase"
                      }
                    >
                      {proposal.userVote}
                    </span>
                  </div>
                )}
              </div>

              {/* Description */}
              <p className="text-xs text-white/60 leading-relaxed">
                {proposal.description}
              </p>

              {/* Metadata Badges */}
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-2.5 pt-1 text-[11px] text-white/50 border-t border-white/[0.04]">
                <div className="flex items-center gap-1.5">
                  <User className="w-3.5 h-3.5 text-white/30 shrink-0" />
                  <span>Author:</span>
                  <code className="text-white/80 font-mono text-[10px]">
                    {proposal.authorNode}
                  </code>
                </div>

                <div className="flex items-center gap-1.5">
                  <Award className="w-3.5 h-3.5 text-white/30 shrink-0" />
                  <span>Required Endorsement:</span>
                  <span
                    className={
                      hasEndorsement
                        ? "text-emerald-400/90 font-medium"
                        : "text-amber-400/90 font-medium"
                    }
                  >
                    {proposal.requiredEndorsement}
                  </span>
                </div>

                <div className="flex items-center gap-1.5">
                  <Clock className="w-3.5 h-3.5 text-white/30 shrink-0" />
                  <span>Time:</span>
                  <span className="text-white/80">
                    {formatTimeRemaining(proposal.expiresAt)}
                  </span>
                </div>
              </div>

              {/* Anti-Hallucination Guard Warning */}
              {!hasEndorsement && (
                <div className="flex items-center gap-2 p-3 rounded-xl bg-amber-500/10 border border-amber-500/20 text-amber-300 text-xs">
                  <Lock className="w-4 h-4 text-amber-400 shrink-0" />
                  <span>
                    Voting Disabled: Current node lacks required endorsement (Requires:{" "}
                    <strong className="font-mono">{proposal.requiredEndorsement}</strong>).
                  </span>
                </div>
              )}

              {/* Visual Progress Bars */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4 pt-2">
                {/* Quorum Progress Bar */}
                <div className="space-y-1.5 p-3.5 rounded-xl bg-black/30 border border-white/5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-white/50 flex items-center gap-1.5">
                      <BarChart3 className="w-3.5 h-3.5 text-blue-400" />
                      Quorum Progress
                    </span>
                    <span className="font-mono text-[11px] text-white/80">
                      {proposal.quorum.current} / {proposal.quorum.required} votes ({quorumPercent}%)
                    </span>
                  </div>
                  <div className="h-2 w-full bg-white/5 rounded-full overflow-hidden">
                    <div
                      className={`h-full transition-all duration-300 ${
                        quorumPercent >= 100 ? "bg-blue-400" : "bg-blue-500/70"
                      }`}
                      style={{ width: `${quorumPercent}%` }}
                    />
                  </div>
                </div>

                {/* Approval Percentage Progress Bar */}
                <div className="space-y-1.5 p-3.5 rounded-xl bg-black/30 border border-white/5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-white/50 flex items-center gap-1.5">
                      <CheckCircle2 className="w-3.5 h-3.5 text-[#39ff14]" />
                      Approval Ratio
                    </span>
                    <span className="font-mono text-[11px] text-white/80">
                      {approvalPercent}% Approval ({proposal.votes.for} For / {proposal.votes.against} Against)
                    </span>
                  </div>
                  <div className="h-2 w-full bg-white/5 rounded-full overflow-hidden flex">
                    <div
                      className="h-full bg-[#39ff14] transition-all duration-300"
                      style={{ width: `${approvalPercent}%` }}
                    />
                    <div
                      className="h-full bg-red-500/70 transition-all duration-300"
                      style={{ width: `${100 - approvalPercent}%` }}
                    />
                  </div>
                </div>
              </div>

              {/* Ballot Action Buttons */}
              <div className="pt-2 flex flex-col sm:flex-row items-center justify-between gap-3 border-t border-white/[0.04]">
                <div className="text-[11px] text-white/40">
                  Total Votes Cast: <strong className="text-white/70 font-mono">{totalVotes}</strong>
                </div>

                <div className="flex items-center gap-2 w-full sm:w-auto">
                  <button
                    type="button"
                    onClick={() => handleVote(proposal.id, "for")}
                    disabled={isVotingDisabled}
                    aria-label={`Vote For on ${proposal.title}`}
                    className={`flex-1 sm:flex-none px-4 py-2 rounded-xl text-xs font-medium transition-all flex items-center justify-center gap-1.5 ${
                      proposal.userVote === "for"
                        ? "bg-[#39ff14]/20 border border-[#39ff14]/50 text-[#39ff14]"
                        : "bg-[#39ff14]/10 border border-[#39ff14]/20 text-[#39ff14] hover:bg-[#39ff14]/20"
                    } disabled:opacity-40 disabled:cursor-not-allowed`}
                  >
                    <CheckCircle2 className="w-3.5 h-3.5" />
                    Vote For ({proposal.votes.for})
                  </button>

                  <button
                    type="button"
                    onClick={() => handleVote(proposal.id, "against")}
                    disabled={isVotingDisabled}
                    aria-label={`Vote Against on ${proposal.title}`}
                    className={`flex-1 sm:flex-none px-4 py-2 rounded-xl text-xs font-medium transition-all flex items-center justify-center gap-1.5 ${
                      proposal.userVote === "against"
                        ? "bg-red-500/20 border border-red-500/50 text-red-400"
                        : "bg-red-500/10 border border-red-500/20 text-red-400 hover:bg-red-500/20"
                    } disabled:opacity-40 disabled:cursor-not-allowed`}
                  >
                    <XCircle className="w-3.5 h-3.5" />
                    Vote Against ({proposal.votes.against})
                  </button>

                  <button
                    type="button"
                    onClick={() => handleVote(proposal.id, "abstain")}
                    disabled={isVotingDisabled}
                    aria-label={`Abstain vote on ${proposal.title}`}
                    className={`flex-1 sm:flex-none px-4 py-2 rounded-xl text-xs font-medium transition-all flex items-center justify-center gap-1.5 ${
                      proposal.userVote === "abstain"
                        ? "bg-amber-500/20 border border-amber-500/50 text-amber-300"
                        : "bg-white/5 border border-white/10 text-white/70 hover:bg-white/10"
                    } disabled:opacity-40 disabled:cursor-not-allowed`}
                  >
                    <MinusCircle className="w-3.5 h-3.5" />
                    Abstain ({proposal.votes.abstain})
                  </button>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: ProposalStatus }) {
  switch (status.toLowerCase()) {
    case "active":
      return (
        <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-[10px] font-medium uppercase tracking-wider bg-emerald-500/10 border border-emerald-500/20 text-emerald-400">
          <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
          Active
        </span>
      );
    case "passed":
      return (
        <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-[10px] font-medium uppercase tracking-wider bg-blue-500/10 border border-blue-500/20 text-blue-400">
          <ShieldCheck className="w-3 h-3 text-blue-400" />
          Passed
        </span>
      );
    case "rejected":
      return (
        <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-[10px] font-medium uppercase tracking-wider bg-red-500/10 border border-red-500/20 text-red-400">
          <XCircle className="w-3 h-3 text-red-400" />
          Rejected
        </span>
      );
    case "expired":
      return (
        <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-[10px] font-medium uppercase tracking-wider bg-amber-500/10 border border-amber-500/20 text-amber-400">
          <Clock className="w-3 h-3 text-amber-400" />
          Expired
        </span>
      );
    default:
      return (
        <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-[10px] font-medium uppercase tracking-wider bg-white/5 border border-white/10 text-white/60">
          {status}
        </span>
      );
  }
}

export default React.memo(DaoGovernancePanel);
