/**
 * @file nodeIdentity.ts
 * @description Gestor de Identidad Soberana de Nodo para la PWA de Xavier (SWAL Mesh).
 *
 * Permite a cualquier usuario operar como nodo descentralizado en el navegador:
 * 1. Genera y almacena localmente par de llaves Ed25519 (WebCrypto).
 * 2. Deriva el `node_id` criptográfico soberano.
 * 3. Ejecuta el protocolo de reto-respuesta (`swal-mesh-signed-nonce-v1`) con el Nodo Génesis en Cloudflare.
 * 4. Gestiona la sesión de nodo y latidos periódicos (heartbeats) para acumular Karma y tokens.
 */

export { isWebAuthnPrfSupported, deriveHardwareSeedWithPrf, DEFAULT_XAVIER_PRF_SALT } from "./webauthnPrf";
export type { HardwareSeedResult } from "./webauthnPrf";

const STORAGE_KEY_KEYPAIR = "swal_node_keypair_v1";
const STORAGE_KEY_SESSION = "swal_node_session_v1";

export interface NodeIdentity {
  nodeId: string;
  publicKeyHex: string;
  createdAt: number;
}

export interface NodeSession {
  nodeId: string;
  token: string;
  karma: number;
  genesisNode: string;
  genesisStatus: string;
  connectedAt: number;
}

export interface ChallengeResponse {
  ok: boolean;
  challenge_id: string;
  nonce_hex: string;
  issued_at: number;
  expires_at: number;
  domain: string;
  genesis_node?: string;
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

async function sha256Hex(data: string | Uint8Array): Promise<string> {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  const digest = await crypto.subtle.digest("SHA-256", bytes as BufferSource);
  return bytesToHex(new Uint8Array(digest));
}

/**
 * Obtiene o inicializa la identidad de nodo local en el navegador.
 */
export async function getOrCreateNodeIdentity(): Promise<{
  identity: NodeIdentity;
  privateKey: CryptoKey;
  publicKey: CryptoKey;
}> {
  const existing = typeof localStorage !== "undefined" ? localStorage.getItem(STORAGE_KEY_KEYPAIR) : null;
  if (existing) {
    try {
      const parsed = JSON.parse(existing);
      const privKey = await crypto.subtle.importKey(
        "jwk",
        parsed.privJwk,
        { name: "Ed25519" },
        false,
        ["sign"]
      );
      const pubKey = await crypto.subtle.importKey(
        "jwk",
        parsed.pubJwk,
        { name: "Ed25519" },
        true,
        ["verify"]
      );
      return {
        identity: {
          nodeId: parsed.nodeId,
          publicKeyHex: parsed.publicKeyHex,
          createdAt: parsed.createdAt,
        },
        privateKey: privKey,
        publicKey: pubKey,
      };
    } catch {
      localStorage.removeItem(STORAGE_KEY_KEYPAIR);
    }
  }

  // Generar nuevo par de claves Ed25519
  const keyPair = (await crypto.subtle.generateKey("Ed25519", true, [
    "sign",
    "verify",
  ])) as CryptoKeyPair;

  const rawPub = await crypto.subtle.exportKey("raw", keyPair.publicKey);
  const pubHex = bytesToHex(new Uint8Array(rawPub));
  const hash = await sha256Hex(new Uint8Array(rawPub));
  const nodeId = `node_${hash.slice(0, 16)}`;

  const privJwk = await crypto.subtle.exportKey("jwk", keyPair.privateKey);
  const pubJwk = await crypto.subtle.exportKey("jwk", keyPair.publicKey);

  const identity: NodeIdentity = {
    nodeId,
    publicKeyHex: pubHex,
    createdAt: Date.now(),
  };

  if (typeof localStorage !== "undefined") {
    localStorage.setItem(
      STORAGE_KEY_KEYPAIR,
      JSON.stringify({
        ...identity,
        privJwk,
        pubJwk,
      })
    );
  }

  return {
    identity,
    privateKey: keyPair.privateKey,
    publicKey: keyPair.publicKey,
  };
}

/**
 * URL base del Nodo Génesis (relativa si está en Cloudflare Pages con functions proxy, o directa).
 */
export function getGenesisEndpoint(): string {
  if (typeof window !== "undefined" && window.location.hostname.includes("swal.network")) {
    return ""; // relativo a través del proxy Pages Functions
  }
  return "https://xaviercloud.swal.network";
}

/**
 * Ejecuta el flujo completo de Login Descentralizado contra el Nodo Génesis:
 * 1. Pide reto al Nodo Génesis (/v1/auth/challenge).
 * 2. Firma el reto canónico localmente en memoria del navegador.
 * 3. Valida en el Edge (/v1/auth/verify).
 * 4. Devuelve la sesión y los puntos de Karma.
 */
export async function performNodeLogin(): Promise<NodeSession> {
  const { identity, privateKey } = await getOrCreateNodeIdentity();
  const endpoint = getGenesisEndpoint();

  // 1. Obtener reto criptográfico del Nodo Génesis
  const challengeRes = await fetch(`${endpoint}/v1/auth/challenge`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
  });

  if (!challengeRes.ok) {
    throw new Error(`Error del Nodo Génesis al solicitar reto: HTTP ${challengeRes.status}`);
  }

  const challenge = (await challengeRes.json()) as ChallengeResponse;
  if (!challenge.ok || !challenge.challenge_id || !challenge.nonce_hex) {
    throw new Error("Respuesta de reto criptográfico malformada");
  }

  // 2. Firmar el payload canónico SWAL:
  // swal-mesh-signed-nonce-v1|challenge_id|nonce_hex|issued_at|expires_at|node_id
  const canonicalPayload = `swal-mesh-signed-nonce-v1|${challenge.challenge_id}|${challenge.nonce_hex}|${challenge.issued_at}|${challenge.expires_at}|${identity.nodeId}`;
  const payloadBytes = new TextEncoder().encode(canonicalPayload);

  const signatureBytes = await crypto.subtle.sign(
    "Ed25519",
    privateKey,
    payloadBytes as BufferSource
  );
  const signatureHex = bytesToHex(new Uint8Array(signatureBytes));

  // 3. Enviar verificación de reto al Nodo Génesis
  const verifyRes = await fetch(`${endpoint}/v1/auth/verify`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      challenge_id: challenge.challenge_id,
      node_id: identity.nodeId,
      public_key_hex: identity.publicKeyHex,
      signature_hex: signatureHex,
    }),
  });

  if (!verifyRes.ok) {
    const errJson = (await verifyRes.json().catch(() => ({}))) as { message?: string };
    throw new Error(errJson.message || `Fallo de verificación en Nodo Génesis (HTTP ${verifyRes.status})`);
  }

  const result = (await verifyRes.json()) as {
    ok: boolean;
    token: string;
    node_id: string;
    karma: number;
    genesis_node: string;
    genesis_status: string;
  };

  const session: NodeSession = {
    nodeId: result.node_id,
    token: result.token,
    karma: result.karma,
    genesisNode: result.genesis_node || "xaviercloud.swal.network",
    genesisStatus: result.genesis_status || "connected",
    connectedAt: Date.now(),
  };

  if (typeof localStorage !== "undefined") {
    localStorage.setItem(STORAGE_KEY_SESSION, JSON.stringify(session));
    localStorage.setItem("xavier_token", session.token);
  }

  return session;
}

/**
 * Envía un latido (heartbeat) periódico al Nodo Génesis para sumar Karma por uptime.
 */
export async function sendNodeHeartbeat(): Promise<{ karma: number; ok: boolean }> {
  if (typeof localStorage === "undefined") return { karma: 0, ok: false };
  const sessionRaw = localStorage.getItem(STORAGE_KEY_SESSION);
  if (!sessionRaw) return { karma: 0, ok: false };

  try {
    const session = JSON.parse(sessionRaw) as NodeSession;
    const endpoint = getGenesisEndpoint();
    const res = await fetch(`${endpoint}/v1/mesh/heartbeat`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${session.token}`,
        "Content-Type": "application/json",
      },
    });

    if (res.ok) {
      const data = (await res.json()) as { karma: number };
      session.karma = data.karma;
      localStorage.setItem(STORAGE_KEY_SESSION, JSON.stringify(session));
      return { karma: data.karma, ok: true };
    }
  } catch {}

  return { karma: 0, ok: false };
}

/**
 * Obtiene la sesión de nodo guardada en el cliente.
 */
export function getSavedNodeSession(): NodeSession | null {
  if (typeof localStorage === "undefined") return null;
  const raw = localStorage.getItem(STORAGE_KEY_SESSION);
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}
