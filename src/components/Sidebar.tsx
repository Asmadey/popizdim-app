import React from "react";
import { useTranslation } from "react-i18next";
import {
  Cog,
  FlaskConical,
  History,
  Info,
  Sparkles,
  Cpu,
  Cloud,
  BookText,
  Mic,
  Volume2,
} from "lucide-react";
import { AppWordmark } from "./icons/AppLogo";
import ModelSelector from "./model-selector";
import { useSettings } from "../hooks/useSettings";
import {
  GeneralSettings,
  AdvancedSettings,
  HistorySettings,
  DebugSettings,
  AboutSettings,
  PostProcessingSettings,
  ModelsSettings,
  CloudSettings,
  TtsSettings,
  DictionarySettings,
} from "./settings";

export type SidebarSection = keyof typeof SECTIONS_CONFIG;

interface IconProps {
  width?: number | string;
  height?: number | string;
  size?: number | string;
  className?: string;
  [key: string]: any;
}

interface SectionConfig {
  labelKey: string;
  icon: React.ComponentType<IconProps>;
  component: React.ComponentType;
  enabled: (settings: any) => boolean;
}

export const SECTIONS_CONFIG = {
  general: {
    labelKey: "sidebar.general",
    icon: Mic,
    component: GeneralSettings,
    enabled: () => true,
  },
  history: {
    labelKey: "sidebar.history",
    icon: History,
    component: HistorySettings,
    enabled: () => true,
  },
  models: {
    labelKey: "sidebar.models",
    icon: Cpu,
    component: ModelsSettings,
    enabled: () => true,
  },
  cloud: {
    labelKey: "sidebar.cloud",
    icon: Cloud,
    component: CloudSettings,
    enabled: () => true,
  },
  tts: {
    labelKey: "sidebar.tts",
    icon: Volume2,
    component: TtsSettings,
    enabled: () => true,
  },
  dictionary: {
    labelKey: "sidebar.dictionary",
    icon: BookText,
    component: DictionarySettings,
    enabled: () => true,
  },
  advanced: {
    labelKey: "sidebar.advanced",
    icon: Cog,
    component: AdvancedSettings,
    enabled: () => true,
  },
  postprocessing: {
    labelKey: "sidebar.postProcessing",
    icon: Sparkles,
    component: PostProcessingSettings,
    enabled: () => true,
  },
  debug: {
    labelKey: "sidebar.debug",
    icon: FlaskConical,
    component: DebugSettings,
    enabled: (settings) => settings?.debug_mode ?? false,
  },
  about: {
    labelKey: "sidebar.about",
    icon: Info,
    component: AboutSettings,
    enabled: () => true,
  },
} as const satisfies Record<string, SectionConfig>;

interface SidebarProps {
  activeSection: SidebarSection;
  onSectionChange: (section: SidebarSection) => void;
}

// Gradient tile behind each nav icon (top color, bottom color)
const TILE_COLORS: Record<string, [string, string]> = {
  general: ["#5aa9ff", "#2563eb"],
  history: ["#ffb155", "#f97316"],
  models: ["#b58bff", "#7c3aed"],
  cloud: ["#6fd3ff", "#0ea5e9"],
  tts: ["#5ee0a0", "#16a34a"],
  dictionary: ["#ffd84d", "#eab308"],
  advanced: ["#a3abb8", "#6b7280"],
  postprocessing: ["#ff7ab6", "#db2777"],
  debug: ["#ff8a7a", "#dc2626"],
  about: ["#9ba3af", "#4b5563"],
};

export const Sidebar: React.FC<SidebarProps> = ({
  activeSection,
  onSectionChange,
}) => {
  const { t } = useTranslation();
  const { settings } = useSettings();

  const availableSections = Object.entries(SECTIONS_CONFIG)
    .filter(([_, config]) => config.enabled(settings))
    .map(([id, config]) => ({ id: id as SidebarSection, ...config }));

  return (
    <div className="glass-panel relative z-20 flex flex-col w-56 shrink-0 my-2 ms-2 items-center px-2">
      {/* Wordmark only, flush with the nav icons below (px-2 + item p-2), sized to
          the full width left between them and the right edge. */}
      <div className="w-full px-2 pt-4 pb-3">
        <AppWordmark height={36} className="block" />
      </div>
      <div className="flex min-h-0 w-full flex-1 flex-col items-center gap-0.5 overflow-y-auto pb-3">
        {availableSections.map((section) => {
          const Icon = section.icon;
          const isActive = activeSection === section.id;

          return (
            <div
              key={section.id}
              className={`flex gap-2.5 items-center px-2 py-1.5 w-full rounded-[10px] cursor-pointer transition-colors ${
                isActive
                  ? "bg-text/10"
                  : "hover:bg-text/5"
              }`}
              onClick={() => onSectionChange(section.id)}
            >
              <span
                className="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-[6px] text-white shadow-[inset_0_0_0_0.5px_#ffffff59,0_1px_2px_#0000003d]"
                style={{
                  backgroundImage: `linear-gradient(to bottom, ${TILE_COLORS[section.id]?.[0] ?? "#9ba3af"}, ${TILE_COLORS[section.id]?.[1] ?? "#4b5563"})`,
                }}
              >
                <Icon width={13} height={13} strokeWidth={2.25} />
              </span>
              <p className="text-sm truncate" title={t(section.labelKey)}>
                {t(section.labelKey)}
              </p>
            </div>
          );
        })}
      </div>
      {/* Active model: same width as the navigation items above */}
      <div className="w-full pb-2">
        <ModelSelector variant="panel" />
      </div>
    </div>
  );
};
