import {
  Check,
  Laptop,
  Moon,
  MousePointerClick,
  Palette,
  Sparkles,
  Sun,
  Terminal,
  Type,
} from "lucide-react";
import React from "react";
import BorderedIcon from "../../components/ui/BorderedIcon";
import SegmentedControl from "../../components/ui/SegmentedControl";
import ThemedButton from "../../components/ui/ThemedButton";
import { useTheme } from "../../lib/theme/theme-provider";
import type { ThemeMode } from "../../lib/theme/types";

export default function AppearancePage() {
  const { theme, setTheme, settings, updateSettings } = useTheme();

  const themes: {
    id: ThemeMode;
    title: string;
    description: string;
    tag?: string;
    icon: React.ReactNode;
    bgHex: string;
    cardHex: string;
    accentHex: string;
  }[] = [
    {
      id: "studio-dark",
      title: "Studio Dark",
      description: "Tono oscuro refinado inspirado en UI de IDEs modernos. Sin bordes agresivos.",
      tag: "Predeterminado",
      icon: <Moon className="w-4 h-4 text-blue-400" />,
      bgHex: "#0d0e10",
      cardHex: "#141518",
      accentHex: "#3b82f6",
    },
    {
      id: "studio-bone",
      title: "Hueso Blanco",
      description: "Modo claro ergonómico en tono hueso cálido. Máxima legibilidad y confort.",
      tag: "Ergonómico",
      icon: <Sun className="w-4 h-4 text-amber-500" />,
      bgHex: "#f7f6f2",
      cardHex: "#ebe8e1",
      accentHex: "#2563eb",
    },
    {
      id: "cyberpunk",
      title: "Xavier Cyberpunk",
      description: "Tema clásico de Xavier con acentos verde neón de alta energía.",
      tag: "Secundario",
      icon: <Terminal className="w-4 h-4 text-[#39ff14]" />,
      bgHex: "#050505",
      cardHex: "#0a0a0a",
      accentHex: "#39ff14",
    },
    {
      id: "system",
      title: "Sistema (Auto)",
      description: "Sincroniza automáticamente con las preferencias de modo oscuro del SO.",
      icon: <Laptop className="w-4 h-4 text-zinc-400" />,
      bgHex: "#101114",
      cardHex: "#1a1b20",
      accentHex: "#3b82f6",
    },
  ];

  return (
    <div className="w-full h-full overflow-y-auto px-4 py-6 md:px-8 space-y-8 text-foreground pb-20">
      {/* Header */}
      <div className="border-b border-white/[0.06] pb-5">
        <div className="flex items-center gap-3">
          <BorderedIcon size="md" active>
            <Palette className="w-5 h-5 text-blue-400" />
          </BorderedIcon>
          <div>
            <h2 className="text-xl font-semibold tracking-tight">Aspecto y Temas</h2>
            <p className="text-xs text-foreground/60 mt-0.5">
              Personaliza el tema visual, tipografía nativa del sistema y micro-interacciones.
            </p>
          </div>
        </div>
      </div>

      {/* Theme Selection Grid */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold tracking-wide uppercase text-foreground/70">
            Tema Visual
          </h3>
          <span className="text-xs text-foreground/50">
            Activo: <strong className="text-foreground capitalize">{theme}</strong>
          </span>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
          {themes.map((t) => {
            const isSelected = theme === t.id;
            return (
              <div
                key={t.id}
                role="button"
                tabIndex={0}
                onClick={() => setTheme(t.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    setTheme(t.id);
                  }
                }}
                className={`
                  relative flex flex-col p-4 rounded-xl border transition-all duration-150 cursor-pointer press-attenuation
                  ${
                    isSelected
                      ? "border-blue-500/50 bg-blue-500/[0.04] shadow-lg shadow-black/20"
                      : "border-white/[0.07] bg-[#141518]/60 hover:bg-[#18191d] hover:border-white/15"
                  }
                `}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-center gap-2.5">
                    <div
                      className="w-4 h-4 rounded-full border border-white/20 shrink-0"
                      style={{ backgroundColor: t.bgHex }}
                    />
                    <span className="text-sm font-semibold text-foreground flex items-center gap-2">
                      {t.title}
                      {t.tag && (
                        <span className="text-[10px] uppercase font-bold tracking-wider px-2 py-0.5 rounded-full bg-white/[0.08] text-foreground/70">
                          {t.tag}
                        </span>
                      )}
                    </span>
                  </div>
                  {isSelected && (
                    <div className="w-5 h-5 rounded-full bg-blue-600 flex items-center justify-center shrink-0">
                      <Check className="w-3.5 h-3.5 text-white stroke-[3]" />
                    </div>
                  )}
                </div>

                <p className="text-xs text-foreground/60 mt-2 leading-relaxed">
                  {t.description}
                </p>

                {/* Color Swatch Preview */}
                <div className="mt-3.5 flex items-center gap-1.5 pt-2 border-t border-white/[0.04]">
                  <div
                    className="w-5 h-5 rounded-md border border-white/10"
                    style={{ backgroundColor: t.bgHex }}
                    title="Fondo"
                  />
                  <div
                    className="w-5 h-5 rounded-md border border-white/10"
                    style={{ backgroundColor: t.cardHex }}
                    title="Superficie"
                  />
                  <div
                    className="w-5 h-5 rounded-md border border-white/10"
                    style={{ backgroundColor: t.accentHex }}
                    title="Acento"
                  />
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Typography and Micro-Interactions */}
      <div className="space-y-4">
        <h3 className="text-sm font-semibold tracking-wide uppercase text-foreground/70">
          Tipografía y Ergonomía
        </h3>

        <div className="rounded-xl border border-white/[0.06] bg-[#141518]/70 divide-y divide-white/[0.05] overflow-hidden">
          {/* System Font */}
          <div className="flex items-center justify-between p-4 hover-attenuation">
            <div className="flex items-center gap-3">
              <BorderedIcon size="sm">
                <Type className="w-4 h-4 text-foreground/70" />
              </BorderedIcon>
              <div>
                <span className="text-sm font-medium">Fuente Nativa del Sistema</span>
                <p className="text-xs text-foreground/50">
                  Usa la tipografía nativa del SO (-apple-system, Segoe UI, Roboto) para máxima nitidez y rendimiento.
                </p>
              </div>
            </div>
            <input
              type="checkbox"
              aria-label="Usar fuente del sistema"
              checked={settings.useSystemFont}
              onChange={(e) => updateSettings({ useSystemFont: e.target.checked })}
              className="w-4 h-4 rounded border-white/20 text-blue-600 focus:ring-blue-500 cursor-pointer"
            />
          </div>

          {/* Bordered Icons */}
          <div className="flex items-center justify-between p-4 hover-attenuation">
            <div className="flex items-center gap-3">
              <BorderedIcon size="sm">
                <Sparkles className="w-4 h-4 text-foreground/70" />
              </BorderedIcon>
              <div>
                <span className="text-sm font-medium">Iconos con Bordes</span>
                <p className="text-xs text-foreground/50">
                  Enmarca los iconos en contenedores sutilmente delineados para mayor definición táctil.
                </p>
              </div>
            </div>
            <input
              type="checkbox"
              aria-label="Habilitar iconos con bordes"
              checked={settings.borderedIcons}
              onChange={(e) => updateSettings({ borderedIcons: e.target.checked })}
              className="w-4 h-4 rounded border-white/20 text-blue-600 focus:ring-blue-500 cursor-pointer"
            />
          </div>

          {/* Attenuation on Press & Hover */}
          <div className="flex items-center justify-between p-4 hover-attenuation">
            <div className="flex items-center gap-3">
              <BorderedIcon size="sm">
                <MousePointerClick className="w-4 h-4 text-foreground/70" />
              </BorderedIcon>
              <div>
                <span className="text-sm font-medium">Atenuación OnPress & Hover</span>
                <p className="text-xs text-foreground/50">
                  Respuesta táctil con micro-escalado suave (active:scale-97) y atenuación de opacidad al interactuar.
                </p>
              </div>
            </div>
            <input
              type="checkbox"
              aria-label="Habilitar atenuación en hover y clic"
              checked={settings.enableAttenuation}
              onChange={(e) => updateSettings({ enableAttenuation: e.target.checked })}
              className="w-4 h-4 rounded border-white/20 text-blue-600 focus:ring-blue-500 cursor-pointer"
            />
          </div>
        </div>
      </div>

      {/* Live Interactive Preview */}
      <div className="space-y-3">
        <h3 className="text-sm font-semibold tracking-wide uppercase text-foreground/70">
          Vista Previa en Vivo de Componentes
        </h3>
        <div className="p-5 rounded-xl border border-white/[0.06] bg-[#141518]/90 space-y-4">
          <div className="flex flex-wrap items-center gap-3">
            <ThemedButton variant="primary">Botón Primario</ThemedButton>
            <ThemedButton variant="secondary">Botón Secundario</ThemedButton>
            <ThemedButton variant="pill" active>
              Píldora Activa
            </ThemedButton>
            <BorderedIcon size="md">
              <Sparkles className="w-4 h-4" />
            </BorderedIcon>
          </div>

          <div className="pt-2">
            <SegmentedControl
              value="queue"
              onChange={() => null}
              options={[
                { value: "queue", label: "Queue" },
                { value: "immediate", label: "Send Immediately" },
              ]}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
