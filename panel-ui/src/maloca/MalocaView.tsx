import { ArrowDown, ArrowUp, LayoutTemplate, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { malocaApi } from "./api";
import "./maloca.css";
import type {
  MalocaPack,
  MalocaSectionId,
  ManagerAction,
  MeshSnapshot,
  MeshTicketOffer,
  NetworkParam,
  Proposal,
  SupportTicket,
} from "./types";
import { useMalocaUi } from "./useMalocaUi";

type Props = {
  onClose?: () => void;
  /** Scaffold: treat local session as manager ACL (no vote weight). */
  isManager?: boolean;
};

export default function MalocaView({ onClose, isManager = true }: Props) {
  const {
    config,
    themeStyle,
    sections,
    editLayout,
    setEditLayout,
    moveSection,
    resetUi,
    softAccent,
  } = useMalocaUi();

  const [tab, setTab] = useState<MalocaSectionId>(
    sections[0]?.id ?? "council",
  );
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [actions, setActions] = useState<ManagerAction[]>([]);
  const [pack, setPack] = useState<MalocaPack | null>(null);
  const [inbox, setInbox] = useState<MeshTicketOffer[]>([]);
  const [backlog, setBacklog] = useState<unknown>(null);
  const [tickets, setTickets] = useState<SupportTicket[]>([]);
  const [params, setParams] = useState<NetworkParam[]>([]);
  const [mesh, setMesh] = useState<MeshSnapshot | null>(null);

  const [reason, setReason] = useState("");
  const [propForm, setPropForm] = useState({
    type: "feature_request",
    title: "",
    body: "",
  });
  const [ticketForm, setTicketForm] = useState({ title: "", body: "" });

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [
        p,
        a,
        pk,
        inboxRes,
        backlogRes,
        support,
        paramsRes,
        meshRes,
      ] = await Promise.all([
        malocaApi.listProposals(),
        malocaApi.listManagerActions(),
        malocaApi.pack(),
        malocaApi.listInbox(),
        malocaApi.backlog(),
        malocaApi.listSupport(),
        malocaApi.params(),
        malocaApi.mesh(),
      ]);
      setProposals(p);
      setActions(a);
      setPack(pk);
      setInbox(inboxRes);
      setBacklog(backlogRes);
      setTickets(support);
      setParams(paramsRes);
      setMesh(meshRes);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Maloca API error");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!sections.find((s) => s.id === tab) && sections[0]) {
      setTab(sections[0].id);
    }
  }, [sections, tab]);

  return (
    <div className="maloca-root absolute inset-0 z-40" style={themeStyle}>
      <link
        rel="stylesheet"
        href="https://fonts.googleapis.com/css2?family=Source+Sans+3:wght@400;500;600&family=IBM+Plex+Mono:wght@400;500&display=swap"
      />
      <div className="maloca-shell relative">
        {onClose && (
          <button
            type="button"
            className="maloca-btn maloca-close"
            onClick={onClose}
            aria-label="Cerrar Maloca"
          >
            <X size={16} />
          </button>
        )}

        <header className="maloca-header">
          <div>
            <h1 className="maloca-brand">{config.copy.title}</h1>
            <p className="maloca-subtitle">{config.copy.subtitle}</p>
          </div>
          <div className="maloca-toolbar">
            <button
              type="button"
              className="maloca-btn"
              disabled={loading}
              onClick={() => void refresh()}
            >
              {loading ? "Cargando…" : "Refresh"}
            </button>
            <button
              type="button"
              className="maloca-btn"
              onClick={() => setEditLayout((v) => !v)}
            >
              <LayoutTemplate size={14} style={{ display: "inline", marginRight: 6 }} />
              {editLayout ? "Runtime" : "Edit layout"}
            </button>
            {editLayout && (
              <>
                <button type="button" className="maloca-btn" onClick={softAccent}>
                  Soften accent
                </button>
                <button type="button" className="maloca-btn" onClick={resetUi}>
                  Reset JSON UI
                </button>
              </>
            )}
          </div>
        </header>

        <p className="maloca-muted">{config.copy.managerNote}</p>

        {error && (
          <div className="maloca-card" style={{ borderColor: "var(--maloca-danger)" }}>
            <span className="maloca-muted">Xavier /maloca: {error}</span>
          </div>
        )}

        <nav className="maloca-nav" aria-label="Maloca sections">
          {sections.map((s) => (
            <div key={s.id} style={{ display: "flex", alignItems: "center", gap: 2 }}>
              {editLayout && (
                <>
                  <button
                    type="button"
                    className="maloca-btn"
                    aria-label={`Move ${s.label} up`}
                    onClick={() => moveSection(s.id, -1)}
                  >
                    <ArrowUp size={12} />
                  </button>
                  <button
                    type="button"
                    className="maloca-btn"
                    aria-label={`Move ${s.label} down`}
                    onClick={() => moveSection(s.id, 1)}
                  >
                    <ArrowDown size={12} />
                  </button>
                </>
              )}
              <button
                type="button"
                data-active={tab === s.id}
                onClick={() => setTab(s.id)}
              >
                {s.label}
              </button>
            </div>
          ))}
        </nav>

        <section className="maloca-panel">
          {tab === "council" && (
            <CouncilPanel
              proposals={proposals}
              actions={actions}
              isManager={isManager}
              reason={reason}
              setReason={setReason}
              onAction={async (type, proposalId) => {
                await malocaApi.managerAction({
                  type,
                  proposalId,
                  reason: reason || type,
                });
                setReason("");
                await refresh();
              }}
            />
          )}
          {tab === "proposals" && (
            <ProposalsPanel
              proposals={proposals}
              form={propForm}
              setForm={setPropForm}
              onCreate={async () => {
                if (!propForm.title.trim()) return;
                await malocaApi.createProposal({
                  type: propForm.type,
                  title: propForm.title.trim(),
                  body: propForm.body.trim(),
                  locked_param: propForm.type === "network_parameter",
                });
                setPropForm({ type: "feature_request", title: "", body: "" });
                await refresh();
              }}
            />
          )}
          {tab === "backlog" && <BacklogPanel pack={pack} backlog={backlog} />}
          {tab === "inbox" && (
            <InboxPanel
              inbox={inbox}
              onClaim={async (id) => {
                await malocaApi.claim(id, "local");
                await refresh();
              }}
              onComplete={async (id) => {
                await malocaApi.complete(id);
                await refresh();
              }}
            />
          )}
          {tab === "support" && (
            <SupportPanel
              tickets={tickets}
              form={ticketForm}
              setForm={setTicketForm}
              onCreate={async () => {
                if (!ticketForm.title.trim()) return;
                await malocaApi.createSupport(ticketForm);
                setTicketForm({ title: "", body: "" });
                await refresh();
              }}
            />
          )}
          {tab === "nodes" && <NodesPanel mesh={mesh} />}
          {tab === "params" && (
            <ParamsPanel params={params} lockedKeys={config.editMode.lockedKeys} />
          )}
          {tab === "docs" && <DocsPanel />}
        </section>
      </div>
    </div>
  );
}

function CouncilPanel({
  proposals,
  actions,
  isManager,
  reason,
  setReason,
  onAction,
}: {
  proposals: Proposal[];
  actions: ManagerAction[];
  isManager: boolean;
  reason: string;
  setReason: (v: string) => void;
  onAction: (
    type: ManagerAction["type"],
    proposalId: string,
  ) => Promise<void>;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <p className="maloca-muted">
        Lectura pública. Voto requiere karma ≥ umbral (param). Gerente puede
        reconsiderar o pedir análisis — <strong>sin peso de voto extra</strong>.
      </p>
      {proposals.map((p) => (
        <article key={p.id} className="maloca-card">
          <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
            <strong>{p.title}</strong>
            <span className="maloca-badge">{p.status}</span>
            {p.locked_param && (
              <span className="maloca-badge maloca-badge-locked">LOCKED_UNTIL_QUORUM</span>
            )}
          </div>
          <p className="maloca-muted" style={{ marginTop: 8 }}>
            {p.body}
          </p>
          <p className="maloca-mono" style={{ marginTop: 4, opacity: 0.7 }}>
            {p.type} · {p.id}
          </p>
          {isManager && p.status === "open" && (
            <div className="maloca-toolbar" style={{ marginTop: 10 }}>
              <input
                className="maloca-input"
                style={{ flex: 1, minWidth: 160 }}
                placeholder="Motivo gerente…"
                value={reason}
                onChange={(e) => setReason(e.target.value)}
              />
              <button
                type="button"
                className="maloca-btn"
                onClick={() => void onAction("request_reconsideration", p.id)}
              >
                Reconsiderar
              </button>
              <button
                type="button"
                className="maloca-btn"
                onClick={() => void onAction("request_scenario_analysis", p.id)}
              >
                Analizar escenarios
              </button>
            </div>
          )}
        </article>
      ))}
      {!proposals.length && <p className="maloca-muted">Sin propuestas.</p>}
      {actions.length > 0 && (
        <div>
          <h4 className="maloca-muted" style={{ marginBottom: 6 }}>
            Acciones gerente
          </h4>
          <ul className="maloca-mono" style={{ listStyle: "none", padding: 0 }}>
            {actions.map((a) => (
              <li key={a.id}>
                {a.type} → {a.proposalId} · {a.reason}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function ProposalsPanel({
  proposals,
  form,
  setForm,
  onCreate,
}: {
  proposals: Proposal[];
  form: { type: string; title: string; body: string };
  setForm: (v: { type: string; title: string; body: string }) => void;
  onCreate: () => Promise<void>;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <form
        className="maloca-card"
        style={{ display: "flex", flexDirection: "column", gap: 10 }}
        onSubmit={(e) => {
          e.preventDefault();
          void onCreate();
        }}
      >
        <h3 style={{ margin: 0, fontSize: "0.95rem" }}>Nueva propuesta</h3>
        <select
          className="maloca-select"
          value={form.type}
          onChange={(e) => setForm({ ...form, type: e.target.value })}
        >
          <option value="feature_request">feature_request</option>
          <option value="parameter_change">parameter_change</option>
          <option value="network_parameter">network_parameter</option>
          <option value="protocol_upgrade">protocol_upgrade</option>
          <option value="general">general</option>
        </select>
        <input
          className="maloca-input"
          placeholder="Título"
          value={form.title}
          onChange={(e) => setForm({ ...form, title: e.target.value })}
        />
        <textarea
          className="maloca-textarea"
          rows={3}
          placeholder="Detalle"
          value={form.body}
          onChange={(e) => setForm({ ...form, body: e.target.value })}
        />
        <button type="submit" className="maloca-btn maloca-btn-primary">
          Publicar
        </button>
      </form>
      <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "grid", gap: 8 }}>
        {proposals.map((p) => (
          <li key={p.id} className="maloca-card">
            <span>{p.title}</span>{" "}
            <span className="maloca-badge">{p.status}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function BacklogPanel({
  pack,
  backlog,
}: {
  pack: MalocaPack | null;
  backlog: unknown;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      {pack && (
        <div style={{ display: "grid", gap: 10, gridTemplateColumns: "repeat(auto-fit,minmax(140px,1fr))" }}>
          <Stat label="Features" value={pack.features_total} />
          <Stat label="Support open" value={pack.support_open} />
          <Stat label="Inbox open" value={pack.inbox_open} />
          <Stat label="Decisions" value={pack.decisions_count} />
        </div>
      )}
      {backlog != null && (
        <pre
          className="maloca-mono"
          style={{
            overflow: "auto",
            padding: 12,
            borderRadius: 8,
            border: "1px solid var(--maloca-border)",
            background: "var(--maloca-bg-muted)",
            margin: 0,
          }}
        >
          {JSON.stringify(backlog, null, 2)}
        </pre>
      )}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="maloca-card">
      <p className="maloca-muted" style={{ fontSize: 10, textTransform: "uppercase", margin: 0 }}>
        {label}
      </p>
      <p style={{ fontSize: "1.25rem", margin: "4px 0 0" }}>{value}</p>
    </div>
  );
}

function InboxPanel({
  inbox,
  onClaim,
  onComplete,
}: {
  inbox: MeshTicketOffer[];
  onClaim: (id: string) => Promise<void>;
  onComplete: (id: string) => Promise<void>;
}) {
  return (
    <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "grid", gap: 10 }}>
      {inbox.map((offer) => (
        <li key={offer.id} className="maloca-card">
          <p style={{ margin: 0 }}>{offer.microtask.title}</p>
          <p className="maloca-muted">{offer.microtask.acceptance}</p>
          <p className="maloca-mono" style={{ opacity: 0.7 }}>
            reward {offer.microtask.reward_hint} · {offer.id}
            {offer.claimed_by ? ` · claimed:${offer.claimed_by}` : ""}
          </p>
          <div className="maloca-toolbar" style={{ marginTop: 8 }}>
            <button type="button" className="maloca-btn" onClick={() => void onClaim(offer.id)}>
              Claim
            </button>
            <button
              type="button"
              className="maloca-btn"
              onClick={() => void onComplete(offer.id)}
            >
              Complete
            </button>
          </div>
        </li>
      ))}
      {!inbox.length && (
        <li className="maloca-muted">Sin offers en inbox.</li>
      )}
    </ul>
  );
}

function SupportPanel({
  tickets,
  form,
  setForm,
  onCreate,
}: {
  tickets: SupportTicket[];
  form: { title: string; body: string };
  setForm: (v: { title: string; body: string }) => void;
  onCreate: () => Promise<void>;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <form
        className="maloca-card"
        style={{ display: "flex", flexDirection: "column", gap: 8 }}
        onSubmit={(e) => {
          e.preventDefault();
          void onCreate();
        }}
      >
        <h3 style={{ margin: 0, fontSize: "0.95rem" }}>Nuevo ticket</h3>
        <input
          className="maloca-input"
          placeholder="Título"
          value={form.title}
          onChange={(e) => setForm({ ...form, title: e.target.value })}
        />
        <textarea
          className="maloca-textarea"
          rows={2}
          placeholder="Detalle"
          value={form.body}
          onChange={(e) => setForm({ ...form, body: e.target.value })}
        />
        <button type="submit" className="maloca-btn maloca-btn-primary">
          Crear
        </button>
      </form>
      <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "grid", gap: 8 }}>
        {tickets.map((t) => (
          <li key={t.id} className="maloca-card">
            <span>{t.title}</span> <span className="maloca-badge">{t.status}</span>
            <p className="maloca-muted">{t.body}</p>
          </li>
        ))}
        {!tickets.length && <li className="maloca-muted">Sin tickets.</li>}
      </ul>
    </div>
  );
}

function NodesPanel({ mesh }: { mesh: MeshSnapshot | null }) {
  if (!mesh) return <p className="maloca-muted">Cargando mesh…</p>;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <p className="maloca-muted">
        Modelo plano: <strong>sin nodos padre</strong>. Wallet ancla N nodos.
        Gerente ACL sin peso de voto (
        <code className="maloca-mono">manager_adds_vote_weight=
        {String(mesh.manager_adds_vote_weight)}</code>
        ).
      </p>
      <div className="maloca-card">
        <h3 style={{ margin: "0 0 8px", fontSize: "0.95rem" }}>Nodos</h3>
        <ul style={{ margin: 0, paddingLeft: 18 }}>
          {mesh.nodes.map((n) => (
            <li key={n.node_id} className="maloca-muted">
              <span className="maloca-mono">{n.node_id}</span> — {n.role}: {n.note}
            </li>
          ))}
        </ul>
      </div>
      <div
        style={{
          display: "grid",
          gap: 10,
          gridTemplateColumns: "repeat(auto-fit,minmax(220px,1fr))",
        }}
      >
        {mesh.meshes.map((m) => (
          <div key={m.id} className="maloca-card">
            <h4 style={{ margin: 0 }}>{m.id}</h4>
            <p className="maloca-badge" style={{ marginTop: 6 }}>
              {m.kind}
            </p>
            <p className="maloca-muted">{m.description}</p>
          </div>
        ))}
      </div>
    </div>
  );
}

function ParamsPanel({
  params,
  lockedKeys,
}: {
  params: NetworkParam[];
  lockedKeys: string[];
}) {
  return (
    <div>
      <p className="maloca-muted" style={{ marginBottom: 12 }}>
        Inscritos por <code className="maloca-mono">lab_genesis</code>. Los LOCKED
        no se desbloquean desde UI/JSON de agentes.
      </p>
      <div className="maloca-card" style={{ padding: 0, overflow: "auto" }}>
        <table className="maloca-table">
          <thead>
            <tr>
              <th>Key</th>
              <th>Default</th>
              <th>Lock</th>
              <th>Notes</th>
            </tr>
          </thead>
          <tbody>
            {params.map((p) => {
              const agentLocked =
                p.locked_until_quorum || lockedKeys.includes(p.key);
              return (
                <tr key={p.key}>
                  <td className="maloca-mono">{p.key}</td>
                  <td className="maloca-mono">{p.default}</td>
                  <td>
                    {agentLocked ? (
                      <span className="maloca-badge maloca-badge-locked">LOCKED</span>
                    ) : (
                      <span className="maloca-badge">open</span>
                    )}
                  </td>
                  <td className="maloca-muted">{p.notes}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function DocsPanel() {
  return (
    <div className="maloca-muted" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <p>
        Host primario: <strong>Xavier panel MalocaView</strong> + API{" "}
        <code className="maloca-mono">/maloca/*</code>.
      </p>
      <p>
        Shell consumidor: <code className="maloca-mono">swal-backoffice</code> vía{" "}
        <code className="maloca-mono">@swal/maloca-client</code>.
      </p>
      <p>PWA Maloca = fase posterior. Synapse permanece frozen. Sin Solana.</p>
      <ul style={{ margin: 0 }}>
        <li>
          <code className="maloca-mono">docs/SWAL/MALOCA_SUPPORT_WORKSPACE.md</code>
        </li>
        <li>
          <code className="maloca-mono">docs/SWAL/NODE_MESH_MANAGER.md</code>
        </li>
        <li>
          <code className="maloca-mono">docs/SWAL/NETWORK_PARAMETERS.md</code>
        </li>
        <li>
          <code className="maloca-mono">docs/SWAL/MALOCA_UI_JSON_CANVAS.md</code>
        </li>
      </ul>
    </div>
  );
}
