// Visual manager for family health records and time-locked / read-once doctor share passes.
import {
  AlertTriangle,
  Calendar,
  Check,
  Clock,
  Copy,
  Download,
  Eye,
  FileText,
  Key,
  Plus,
  QrCode,
  ShieldAlert,
  Trash2,
  User,
  UserCheck,
  X,
} from "lucide-react";
import React, { useMemo, useState } from "react";
import { QrCodeDisplay } from "../QrCodeDisplay";

export interface LabAttachment {
  id: string;
  name: string;
  fileSize: string;
  type: string;
}

export interface MedicalEpisode {
  id: string;
  familyMember: string;
  date: string;
  diagnosis: string;
  doctor: string;
  severity: "low" | "medium" | "high";
  notes: string;
  attachments: LabAttachment[];
}

export interface AccessToken {
  id: string;
  token: string;
  recipientDoctor: string;
  passType: "1-hour" | "read-once";
  scope: string;
  createdAt: string;
  expiresAt: string;
  status: "active" | "revoked" | "expired";
}

const DEFAULT_EPISODES: MedicalEpisode[] = [
  {
    id: "ep-001",
    familyMember: "Alice Smith",
    date: "2026-03-01",
    diagnosis: "Acute Bronchitis",
    doctor: "Dr. Evelyn Reed",
    severity: "medium",
    notes: "Prescribed 7-day amoxicillin course. Follow-up if fever persists.",
    attachments: [
      { id: "att-101", name: "Chest_XRay_Results.pdf", fileSize: "2.4 MB", type: "application/pdf" },
      { id: "att-102", name: "Blood_Panel_Lab.pdf", fileSize: "1.1 MB", type: "application/pdf" },
    ],
  },
  {
    id: "ep-002",
    familyMember: "Bob Smith",
    date: "2026-02-14",
    diagnosis: "Hypertension Checkup",
    doctor: "Dr. Marcus Vance",
    severity: "low",
    notes: "BP stable at 122/80. Continue daily lisinopril 10mg.",
    attachments: [
      { id: "att-103", name: "ECG_Telemetry_Report.pdf", fileSize: "3.8 MB", type: "application/pdf" },
    ],
  },
  {
    id: "ep-003",
    familyMember: "Charlie Smith",
    date: "2026-01-20",
    diagnosis: "Pediatric Wellness & Vaccination",
    doctor: "Dr. Sarah Chen",
    severity: "low",
    notes: "Annual checkup complete. Administered booster vaccines.",
    attachments: [
      { id: "att-104", name: "Immunization_Record.pdf", fileSize: "850 KB", type: "application/pdf" },
    ],
  },
];

const DEFAULT_TOKENS: AccessToken[] = [
  {
    id: "tok-001",
    token: "pass_live_8f93a12b4d",
    recipientDoctor: "Dr. Evelyn Reed",
    passType: "1-hour",
    scope: "Alice Smith - All Records",
    createdAt: "2026-03-06 10:00:00",
    expiresAt: "2026-03-06 11:00:00",
    status: "active",
  },
  {
    id: "tok-002",
    token: "pass_once_3c92e10a",
    recipientDoctor: "Dr. Marcus Vance",
    passType: "read-once",
    scope: "Bob Smith - Hypertension Checkup",
    createdAt: "2026-03-05 14:30:00",
    expiresAt: "2026-03-05 14:35:00",
    status: "revoked",
  },
];

export interface FamilyHealthRecordsProps {
  initialEpisodes?: MedicalEpisode[];
  initialTokens?: AccessToken[];
}

export function FamilyHealthRecords({
  initialEpisodes = DEFAULT_EPISODES,
  initialTokens = DEFAULT_TOKENS,
}: FamilyHealthRecordsProps) {
  const [episodes] = useState<MedicalEpisode[]>(initialEpisodes);
  const [tokens, setTokens] = useState<AccessToken[]>(initialTokens);
  const [selectedMember, setSelectedMember] = useState<string>("All");
  const [isShareModalOpen, setIsShareModalOpen] = useState<boolean>(false);

  // Share Modal Form State
  const [passType, setPassType] = useState<"1-hour" | "read-once">("1-hour");
  const [recipientDoctor, setRecipientDoctor] = useState<string>("");
  const [targetScope, setTargetScope] = useState<string>("All Members");
  const [generatedPass, setGeneratedPass] = useState<AccessToken | null>(null);
  const [copiedToken, setCopiedToken] = useState<boolean>(false);

  // Unique family members for filtering
  const familyMembers = useMemo(() => {
    const members = Array.from(new Set(episodes.map((ep) => ep.familyMember)));
    return ["All", ...members];
  }, [episodes]);

  // Filtered episodes based on selection
  const filteredEpisodes = useMemo(() => {
    if (selectedMember === "All") return episodes;
    return episodes.filter((ep) => ep.familyMember === selectedMember);
  }, [episodes, selectedMember]);

  const activeTokens = useMemo(
    () => tokens.filter((t) => t.status === "active"),
    [tokens]
  );
  const revokedTokens = useMemo(
    () => tokens.filter((t) => t.status !== "active"),
    [tokens]
  );

  const handleOpenShareModal = (member?: string) => {
    if (member && member !== "All") {
      setTargetScope(`${member} - All Records`);
    } else {
      setTargetScope("All Members");
    }
    setRecipientDoctor("");
    setGeneratedPass(null);
    setCopiedToken(false);
    setIsShareModalOpen(true);
  };

  const handleGeneratePass = (e: React.FormEvent) => {
    e.preventDefault();
    const docName = recipientDoctor.trim() || "Attending Physician";
    const now = new Date();
    const expiry = new Date(
      now.getTime() + (passType === "1-hour" ? 60 * 60 * 1000 : 5 * 60 * 1000)
    );

    const newToken: AccessToken = {
      id: `tok-${Date.now()}`,
      token: `pass_${passType === "1-hour" ? "live" : "once"}_${Math.random()
        .toString(36)
        .substring(2, 10)}`,
      recipientDoctor: docName,
      passType,
      scope: targetScope,
      createdAt: now.toISOString().replace("T", " ").substring(0, 19),
      expiresAt: expiry.toISOString().replace("T", " ").substring(0, 19),
      status: "active",
    };

    setTokens((prev) => [newToken, ...prev]);
    setGeneratedPass(newToken);
  };

  const handleRevokeToken = (tokenId: string) => {
    setTokens((prev) =>
      prev.map((t) => (t.id === tokenId ? { ...t, status: "revoked" } : t))
    );
  };

  const handleCopyPassUrl = (tokenStr: string) => {
    const shareUrl = `https://xavier.health/share/${tokenStr}`;
    navigator.clipboard?.writeText(shareUrl);
    setCopiedToken(true);
    setTimeout(() => setCopiedToken(false), 2000);
  };

  const getSeverityBadge = (severity: MedicalEpisode["severity"]) => {
    switch (severity) {
      case "high":
        return "bg-red-500/20 text-red-400 border-red-500/30";
      case "medium":
        return "bg-amber-500/20 text-amber-400 border-amber-500/30";
      case "low":
        return "bg-emerald-500/20 text-emerald-400 border-emerald-500/30";
    }
  };

  return (
    <div className="space-y-8 bg-[#050505]/60 border border-white/10 rounded-2xl p-6 shadow-xl backdrop-blur-md">
      {/* Top Header & Security Banner */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4 pb-6 border-b border-white/10">
        <div className="flex items-center gap-4">
          <div className="p-3 bg-emerald-500/10 border border-emerald-500/20 rounded-xl text-emerald-400">
            <UserCheck className="w-6 h-6" aria-hidden="true" />
          </div>
          <div>
            <h2 className="text-xl font-bold tracking-tight text-white">
              Family Health Records Manager
            </h2>
            <p className="text-xs text-white/50 mt-1">
              Encrypted visual manager for medical episodes, diagnoses, lab attachments, and time-locked doctor share passes.
            </p>
          </div>
        </div>

        <button
          type="button"
          onClick={() => handleOpenShareModal()}
          aria-label="Share with Doctor"
          className="px-4 py-2.5 bg-emerald-500 hover:bg-emerald-400 text-black font-semibold text-xs rounded-xl transition-all duration-200 flex items-center gap-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400 shadow-[0_0_12px_rgba(16,185,129,0.3)] self-start md:self-auto"
        >
          <Key className="w-4 h-4" aria-hidden="true" />
          Share with Doctor
        </button>
      </div>

      {/* Security Warning Notice */}
      <div className="bg-amber-500/10 border border-amber-500/30 rounded-xl p-4 flex items-start gap-3 text-amber-300 text-xs">
        <ShieldAlert className="w-5 h-5 text-amber-400 shrink-0 mt-0.5" aria-hidden="true" />
        <div>
          <span className="font-semibold block text-amber-200 mb-0.5">
            Security & Compliance Warning: Sensitive Medical Information
          </span>
          Family health records contain Protected Health Information (PHI). Active access tokens allow doctors temporary or single-use retrieval. All access passes automatically log actions and expire strictly according to policy.
        </div>
      </div>

      {/* Filter Tabs */}
      <div className="flex items-center gap-2 overflow-x-auto pb-1">
        <span className="text-xs text-white/50 mr-2 flex items-center gap-1 font-medium">
          <User className="w-3.5 h-3.5" aria-hidden="true" /> Filter Member:
        </span>
        {familyMembers.map((member) => (
          <button
            key={member}
            type="button"
            onClick={() => setSelectedMember(member)}
            className={`px-3 py-1.5 text-xs rounded-lg font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400/50 ${
              selectedMember === member
                ? "bg-emerald-500/20 border border-emerald-500/40 text-emerald-300"
                : "bg-white/5 border border-white/5 text-white/70 hover:bg-white/10"
            }`}
          >
            {member}
          </button>
        ))}
      </div>

      {/* Visual Records Grid */}
      <div className="space-y-4">
        <h3 className="text-sm font-semibold uppercase tracking-wider text-white/60 flex items-center gap-2">
          <FileText className="w-4 h-4 text-emerald-400" aria-hidden="true" />
          Medical Episodes & Lab Attachments ({filteredEpisodes.length})
        </h3>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {filteredEpisodes.map((ep) => (
            <div
              key={ep.id}
              className="bg-white/5 border border-white/10 rounded-xl p-4 flex flex-col justify-between hover:border-white/20 transition-all space-y-4"
            >
              <div>
                <div className="flex items-start justify-between gap-2 mb-2">
                  <div>
                    <span className="text-xs font-semibold text-emerald-400 block">
                      {ep.familyMember}
                    </span>
                    <h4 className="text-base font-bold text-white leading-snug">
                      {ep.diagnosis}
                    </h4>
                  </div>
                  <span
                    className={`px-2 py-0.5 text-[10px] uppercase font-bold rounded-full border ${getSeverityBadge(
                      ep.severity
                    )}`}
                  >
                    {ep.severity}
                  </span>
                </div>

                <div className="flex items-center gap-4 text-xs text-white/50 mb-3">
                  <span className="flex items-center gap-1">
                    <Calendar className="w-3.5 h-3.5 text-white/40" aria-hidden="true" />
                    {ep.date}
                  </span>
                  <span>{ep.doctor}</span>
                </div>

                <p className="text-xs text-white/80 bg-black/40 p-2.5 rounded-lg border border-white/5 mb-3 leading-relaxed">
                  {ep.notes}
                </p>
              </div>

              {/* Lab Attachments */}
              <div className="pt-3 border-t border-white/10 space-y-2">
                <span className="text-[11px] font-medium text-white/50 block">
                  Lab Attachments ({ep.attachments.length}):
                </span>
                <div className="space-y-1.5">
                  {ep.attachments.map((att) => (
                    <div
                      key={att.id}
                      className="flex items-center justify-between text-xs bg-white/5 hover:bg-white/10 border border-white/5 rounded-lg px-2.5 py-1.5 text-white/90 transition-colors"
                    >
                      <span className="truncate max-w-[180px]" title={att.name}>
                        {att.name}
                      </span>
                      <span className="text-[10px] font-mono text-white/40">
                        {att.fileSize}
                      </span>
                    </div>
                  ))}
                </div>

                <button
                  type="button"
                  onClick={() => handleOpenShareModal(ep.familyMember)}
                  aria-label={`Share ${ep.familyMember} records with doctor`}
                  className="w-full mt-2 py-1.5 px-3 bg-white/5 hover:bg-emerald-500/20 border border-white/10 hover:border-emerald-500/40 text-emerald-400 text-xs font-semibold rounded-lg transition-colors flex items-center justify-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400"
                >
                  <Key className="w-3.5 h-3.5" aria-hidden="true" />
                  Share Episode Pass
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Access Tokens Audit Table */}
      <div className="pt-6 border-t border-white/10 space-y-4">
        <h3 className="text-sm font-semibold uppercase tracking-wider text-white/60 flex items-center gap-2">
          <Clock className="w-4 h-4 text-emerald-400" aria-hidden="true" />
          Active Doctor Access Tokens ({activeTokens.length})
        </h3>

        {activeTokens.length === 0 ? (
          <div className="bg-white/5 border border-white/5 rounded-xl p-6 text-center text-xs text-white/40">
            No active share tokens found. Generate a time-locked or read-once pass to grant doctor access.
          </div>
        ) : (
          <div className="overflow-x-auto rounded-xl border border-white/10">
            <table className="w-full text-left text-xs text-white/80">
              <thead className="bg-white/5 text-white/50 text-[11px] uppercase border-b border-white/10">
                <tr>
                  <th scope="col" className="p-3">Token Pass</th>
                  <th scope="col" className="p-3">Recipient Doctor</th>
                  <th scope="col" className="p-3">Pass Type</th>
                  <th scope="col" className="p-3">Scope</th>
                  <th scope="col" className="p-3">Expires At</th>
                  <th scope="col" className="p-3 text-right">Action</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/5 font-mono">
                {activeTokens.map((tok) => (
                  <tr key={tok.id} className="hover:bg-white/5 transition-colors">
                    <td className="p-3 font-bold text-emerald-400">{tok.token}</td>
                    <td className="p-3 text-white font-sans">{tok.recipientDoctor}</td>
                    <td className="p-3">
                      <span className="px-2 py-0.5 rounded-full bg-emerald-500/20 text-emerald-300 text-[10px]">
                        {tok.passType}
                      </span>
                    </td>
                    <td className="p-3 text-white/70 font-sans">{tok.scope}</td>
                    <td className="p-3 text-white/50">{tok.expiresAt}</td>
                    <td className="p-3 text-right">
                      <button
                        type="button"
                        onClick={() => handleRevokeToken(tok.id)}
                        aria-label={`Revoke token ${tok.token}`}
                        className="px-2.5 py-1 bg-red-500/20 hover:bg-red-500/30 border border-red-500/30 text-red-300 rounded-md font-sans text-xs transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-400"
                      >
                        Revoke Access
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Revocation History Table */}
      {revokedTokens.length > 0 && (
        <div className="pt-4 space-y-3">
          <h4 className="text-xs font-semibold uppercase tracking-wider text-white/40">
            Revocation & Expiration History ({revokedTokens.length})
          </h4>
          <div className="overflow-x-auto rounded-xl border border-white/5 bg-black/30">
            <table className="w-full text-left text-xs text-white/50">
              <thead className="bg-white/5 text-white/40 text-[10px] uppercase border-b border-white/5">
                <tr>
                  <th scope="col" className="p-2.5">Token</th>
                  <th scope="col" className="p-2.5">Doctor</th>
                  <th scope="col" className="p-2.5">Status</th>
                  <th scope="col" className="p-2.5">Scope</th>
                  <th scope="col" className="p-2.5">Created At</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/5 font-mono">
                {revokedTokens.map((tok) => (
                  <tr key={tok.id}>
                    <td className="p-2.5 line-through">{tok.token}</td>
                    <td className="p-2.5 font-sans">{tok.recipientDoctor}</td>
                    <td className="p-2.5">
                      <span className="px-2 py-0.5 rounded-full bg-red-500/10 text-red-400 text-[10px] uppercase">
                        {tok.status}
                      </span>
                    </td>
                    <td className="p-2.5 font-sans">{tok.scope}</td>
                    <td className="p-2.5 text-white/30">{tok.createdAt}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Share Modal */}
      {isShareModalOpen && (
        <div
          className="fixed inset-0 z-50 bg-black/80 backdrop-blur-sm flex items-center justify-center p-4"
          role="dialog"
          aria-modal="true"
          aria-labelledby="share-modal-title"
        >
          <div className="bg-[#0b0f14] border border-white/15 rounded-2xl w-full max-w-lg p-6 shadow-2xl relative space-y-6 max-h-[90vh] overflow-y-auto">
            <button
              type="button"
              onClick={() => setIsShareModalOpen(false)}
              aria-label="Close share modal"
              className="absolute top-4 right-4 text-white/50 hover:text-white transition-colors p-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400 rounded-lg"
            >
              <X className="w-5 h-5" aria-hidden="true" />
            </button>

            <div>
              <h3 id="share-modal-title" className="text-lg font-bold text-white flex items-center gap-2">
                <Key className="w-5 h-5 text-emerald-400" aria-hidden="true" />
                Generate Doctor Share Pass
              </h3>
              <p className="text-xs text-white/50 mt-1">
                Issue a time-locked or single-use read-once access pass for attending physicians.
              </p>
            </div>

            {/* Modal Security Warning */}
            <div className="bg-amber-500/15 border border-amber-500/40 rounded-xl p-3.5 flex items-start gap-3 text-xs text-amber-200">
              <AlertTriangle className="w-5 h-5 text-amber-400 shrink-0 mt-0.5" aria-hidden="true" />
              <div>
                <span className="font-bold text-amber-100 block mb-0.5">
                  SECURITY WARNING: PHI Data Transfer
                </span>
                Sharing health records grants temporary access to medical history. Verify the attending doctor's identity prior to transmitting link or QR code.
              </div>
            </div>

            {!generatedPass ? (
              <form onSubmit={handleGeneratePass} className="space-y-4">
                <div>
                  <label htmlFor="doctor-name" className="block text-xs font-semibold text-white/80 mb-1">
                    Attending Physician / Doctor Name:
                  </label>
                  <input
                    id="doctor-name"
                    type="text"
                    required
                    placeholder="e.g. Dr. Evelyn Reed"
                    value={recipientDoctor}
                    onChange={(e) => setRecipientDoctor(e.target.value)}
                    className="w-full bg-black/50 border border-white/15 focus:border-emerald-400/60 rounded-xl px-3.5 py-2 text-xs text-white outline-none"
                  />
                </div>

                <div>
                  <label htmlFor="pass-scope" className="block text-xs font-semibold text-white/80 mb-1">
                    Record Scope:
                  </label>
                  <select
                    id="pass-scope"
                    value={targetScope}
                    onChange={(e) => setTargetScope(e.target.value)}
                    className="w-full bg-black/50 border border-white/15 focus:border-emerald-400/60 rounded-xl px-3.5 py-2 text-xs text-white outline-none"
                  >
                    <option value="All Members">All Family Members</option>
                    {familyMembers
                      .filter((m) => m !== "All")
                      .map((m) => (
                        <option key={m} value={`${m} - All Records`}>
                          {m} - All Records
                        </option>
                      ))}
                  </select>
                </div>

                <div>
                  <label htmlFor="pass-type-selector" className="block text-xs font-semibold text-white/80 mb-2">
                    Pass Type & Time Limit:
                  </label>
                  <div id="pass-type-selector" className="grid grid-cols-2 gap-3">
                    <button
                      type="button"
                      onClick={() => setPassType("1-hour")}
                      className={`p-3 rounded-xl border text-left transition-all ${
                        passType === "1-hour"
                          ? "bg-emerald-500/20 border-emerald-500 text-emerald-300"
                          : "bg-white/5 border-white/10 text-white/60 hover:bg-white/10"
                      }`}
                    >
                      <span className="block font-bold text-xs mb-0.5">1-Hour Time-Locked</span>
                      <span className="block text-[10px] opacity-70">
                        Valid for 60 minutes after generation.
                      </span>
                    </button>

                    <button
                      type="button"
                      onClick={() => setPassType("read-once")}
                      className={`p-3 rounded-xl border text-left transition-all ${
                        passType === "read-once"
                          ? "bg-emerald-500/20 border-emerald-500 text-emerald-300"
                          : "bg-white/5 border-white/10 text-white/60 hover:bg-white/10"
                      }`}
                    >
                      <span className="block font-bold text-xs mb-0.5">Read-Once Pass</span>
                      <span className="block text-[10px] opacity-70">
                        Self-destructs immediately after 1 view.
                      </span>
                    </button>
                  </div>
                </div>

                <div className="pt-2 flex justify-end gap-3">
                  <button
                    type="button"
                    onClick={() => setIsShareModalOpen(false)}
                    className="px-4 py-2 bg-white/5 hover:bg-white/10 text-white/70 text-xs rounded-xl transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    className="px-4 py-2 bg-emerald-500 hover:bg-emerald-400 text-black font-semibold text-xs rounded-xl transition-colors shadow-[0_0_12px_rgba(16,185,129,0.3)]"
                  >
                    Generate Pass & QR Code
                  </button>
                </div>
              </form>
            ) : (
              <div className="space-y-4 text-center">
                <div className="p-4 bg-emerald-500/10 border border-emerald-500/30 rounded-2xl inline-block w-full">
                  <span className="text-xs uppercase font-bold text-emerald-400 block mb-1">
                    Pass Created Successfully
                  </span>
                  <p className="text-xs text-white/70">
                    Recipient: <strong className="text-white">{generatedPass.recipientDoctor}</strong> ({generatedPass.passType})
                  </p>
                </div>

                {/* QR Code */}
                <div className="flex flex-col items-center justify-center p-2">
                  <QrCodeDisplay value={`https://xavier.health/share/${generatedPass.token}`} />
                  <span className="text-[11px] font-mono text-white/50 mt-2">
                    Scan QR code with doctor device
                  </span>
                </div>

                {/* Copy Link Input */}
                <div className="flex items-center gap-2">
                  <input
                    type="text"
                    readOnly
                    value={`https://xavier.health/share/${generatedPass.token}`}
                    className="flex-1 bg-black/60 border border-white/15 rounded-xl px-3 py-2 font-mono text-xs text-white/80 outline-none"
                  />
                  <button
                    type="button"
                    onClick={() => handleCopyPassUrl(generatedPass.token)}
                    aria-label="Copy Doctor Share Link"
                    className="px-3 py-2 bg-emerald-500 hover:bg-emerald-400 text-black font-semibold text-xs rounded-xl transition-colors flex items-center gap-1.5"
                  >
                    {copiedToken ? (
                      <>
                        <Check className="w-3.5 h-3.5" aria-hidden="true" /> Copied
                      </>
                    ) : (
                      <>
                        <Copy className="w-3.5 h-3.5" aria-hidden="true" /> Copy
                      </>
                    )}
                  </button>
                </div>

                <div className="pt-3 border-t border-white/10">
                  <button
                    type="button"
                    onClick={() => setIsShareModalOpen(false)}
                    className="w-full py-2 bg-white/10 hover:bg-white/20 text-white text-xs font-medium rounded-xl transition-colors"
                  >
                    Done
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

export default FamilyHealthRecords;
