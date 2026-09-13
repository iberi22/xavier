/**
 * ragProfileStore.svelte.ts — Svelte 5 Runes store for Visual RAG Profile Onboarding Wizard
 *
 * Manages selected RAG profiles (Legal, Code, Video, Photos, Audio),
 * storage quota slider state (10 GB - 500 GB), Zero-Cloud privacy mode,
 * estimated RAM and disk resource allocations, and API client synchronisation
 * with Xavier endpoint `/v1/maloca/config/profiles`.
 */

// Global fallback for Svelte 5 runes when running in uncompiled test environments
if (typeof (globalThis as any).$state === "undefined") {
  (globalThis as any).$state = <T>(init: T): T => init;
}
if (typeof (globalThis as any).$derived === "undefined") {
  (globalThis as any).$derived = {
    by: <T>(fn: () => T): T => {
      try {
        return fn();
      } catch {
        return undefined as unknown as T;
      }
    },
  };
}

export interface RagProfileOption {
  id: "legal" | "code" | "video" | "photos" | "audio";
  title: string;
  category: string;
  icon: string;
  description: string;
  estimatedRamMbPerGb: number; // RAM overhead in MB per GB allocated storage
  recommendedBaseRamMb: number;
}

export const RAG_PROFILES: RagProfileOption[] = [
  {
    id: "legal",
    title: "Documentos Legales & Fiscales",
    category: "Documentos",
    icon: "⚖️",
    description: "PDFs, Contratos, Minutas, Jurisprudencia y normatividad legal con extracción de metadatos.",
    estimatedRamMbPerGb: 8,
    recommendedBaseRamMb: 512,
  },
  {
    id: "code",
    title: "Código Fuente & AST",
    category: "Código",
    icon: "💻",
    description: "Rust, TypeScript, Python, dependencias y grafo de símbolos AST con Tree-sitter.",
    estimatedRamMbPerGb: 12,
    recommendedBaseRamMb: 1024,
  },
  {
    id: "video",
    title: "Video & Escenas",
    category: "Multimedia",
    icon: "🎥",
    description: "Grabaciones de audiencias, inspecciones de campo, timecodes e incrustaciones visuales.",
    estimatedRamMbPerGb: 16,
    recommendedBaseRamMb: 2048,
  },
  {
    id: "photos",
    title: "Fotos & Evidencias Visuales",
    category: "Multimedia",
    icon: "🖼️",
    description: "Imágenes de inspección, extracción EXIF, OCR y catalogación de evidencia visual.",
    estimatedRamMbPerGb: 10,
    recommendedBaseRamMb: 768,
  },
  {
    id: "audio",
    title: "Audio & Dictados",
    category: "Voz",
    icon: "🎙️",
    description: "Dictados de voz a texto con Whisper, transcripción y diarización de interlocutores.",
    estimatedRamMbPerGb: 14,
    recommendedBaseRamMb: 1536,
  },
];

export interface SaveProfilePayload {
  selectedProfiles: string[];
  storageGb: number;
  zeroCloud: boolean;
  timestamp: string;
}

export interface SaveProfileResult {
  success: boolean;
  message: string;
  nodeConfigId?: string;
}

export function useRagProfileStore() {
  let selectedProfiles = $state<string[]>(["legal", "code"]);
  let storageGb = $state<number>(50);
  let zeroCloud = $state<boolean>(true);
  let isSaving = $state<boolean>(false);
  let saveStatus = $state<{ success: boolean; message: string } | null>(null);

  // Helper functions for dynamic calculations
  function calculateEstimatedRamMb(): number {
    if (selectedProfiles.length === 0) return 256;

    let baseRam = 0;
    let totalPerGb = 0;

    for (const profileId of selectedProfiles) {
      const profile = RAG_PROFILES.find((p) => p.id === profileId);
      if (profile) {
        baseRam = Math.max(baseRam, profile.recommendedBaseRamMb);
        totalPerGb += profile.estimatedRamMbPerGb;
      }
    }

    return Math.round(baseRam + storageGb * totalPerGb);
  }

  function calculateFormattedRamEstimate(): string {
    const ram = calculateEstimatedRamMb();
    if (ram >= 1024) {
      return `${(ram / 1024).toFixed(1)} GB`;
    }
    return `${ram} MB`;
  }

  function calculateRecommendedNodeSpecs() {
    const ramGb = Math.ceil(calculateEstimatedRamMb() / 1024);
    return {
      minCpuCores: selectedProfiles.length >= 3 ? 4 : 2,
      minRamGb: Math.max(2, ramGb),
      diskAllocationGb: storageGb,
    };
  }

  // Derived calculation: total estimated RAM allocation in MB
  const estimatedRamMb = $derived.by(() => calculateEstimatedRamMb());

  // Derived calculation: total estimated RAM formatted in GB or MB
  const formattedRamEstimate = $derived.by(() => calculateFormattedRamEstimate());

  // Derived calculation: recommended minimum node specs
  const recommendedNodeSpecs = $derived.by(() => calculateRecommendedNodeSpecs());

  function toggleProfile(profileId: string) {
    if (selectedProfiles.includes(profileId)) {
      selectedProfiles = selectedProfiles.filter((id) => id !== profileId);
    } else {
      selectedProfiles = [...selectedProfiles, profileId];
    }
  }

  function setStorageGb(value: number) {
    storageGb = Math.min(500, Math.max(10, value));
  }

  function setZeroCloud(enabled: boolean) {
    zeroCloud = enabled;
  }

  function resetSelection() {
    selectedProfiles = ["legal", "code"];
    storageGb = 50;
    zeroCloud = true;
    saveStatus = null;
  }

  async function saveProfiles(apiBaseUrl: string = ""): Promise<SaveProfileResult> {
    isSaving = true;
    saveStatus = null;

    const payload: SaveProfilePayload = {
      selectedProfiles: [...selectedProfiles],
      storageGb,
      zeroCloud,
      timestamp: new Date().toISOString(),
    };

    try {
      const endpoint = `${apiBaseUrl.replace(/\/$/, "")}/v1/maloca/config/profiles`;
      const response = await fetch(endpoint, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify(payload),
      });

      if (!response.ok) {
        throw new Error(`HTTP Error ${response.status}: ${response.statusText}`);
      }

      const data = (await response.json()) as { success?: boolean; message?: string; id?: string };
      const result: SaveProfileResult = {
        success: data.success ?? true,
        message: data.message ?? "Configuración de perfiles RAG guardada exitosamente.",
        nodeConfigId: data.id,
      };

      saveStatus = { success: true, message: result.message };
      return result;
    } catch (err: unknown) {
      const errorMessage = err instanceof Error ? err.message : "Error al conectar con el nodo Xavier";
      const result: SaveProfileResult = {
        success: false,
        message: errorMessage,
      };
      saveStatus = { success: false, message: errorMessage };
      return result;
    } finally {
      isSaving = false;
    }
  }

  return {
    // Getters / state
    get selectedProfiles() {
      return selectedProfiles;
    },
    get storageGb() {
      return storageGb;
    },
    get zeroCloud() {
      return zeroCloud;
    },
    get isSaving() {
      return isSaving;
    },
    get saveStatus() {
      return saveStatus;
    },
    get estimatedRamMb() {
      return calculateEstimatedRamMb();
    },
    get formattedRamEstimate() {
      return calculateFormattedRamEstimate();
    },
    get recommendedNodeSpecs() {
      return calculateRecommendedNodeSpecs();
    },

    // Actions
    toggleProfile,
    setStorageGb,
    setZeroCloud,
    resetSelection,
    saveProfiles,
  };
}
