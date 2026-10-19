import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Button } from "../../ui/Button";
import { AppDataDirectory } from "../AppDataDirectory";
import { AppLanguageSelector } from "../AppLanguageSelector";
import { ShowWhatsNewOnUpdate } from "../ShowWhatsNewOnUpdate";
import { ThemeSelector } from "../ThemeSelector";
import { LogDirectory } from "../debug";

const AUTHOR_LINKS = [
  {
    labelKey: "settings.about.author.telegram",
    text: "https://t.me/AImademyday",
    url: "https://t.me/AImademyday",
  },
  {
    labelKey: "settings.about.author.site",
    text: "proshinsky.com",
    url: "https://proshinsky.com",
  },
] as const;

export const AboutSettings: React.FC = () => {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");

  useEffect(() => {
    const fetchVersion = async () => {
      try {
        const appVersion = await getVersion();
        setVersion(appVersion);
      } catch (error) {
        console.error("Failed to get app version:", error);
        setVersion("0.1.2");
      }
    };

    fetchVersion();
  }, []);

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.about.title")}>
        <AppLanguageSelector descriptionMode="tooltip" grouped={true} />
        <ThemeSelector descriptionMode="tooltip" grouped={true} />
        <SettingContainer
          title={t("settings.about.version.title")}
          description={t("settings.about.version.description")}
          grouped={true}
        >
          {/* eslint-disable-next-line i18next/no-literal-string */}
          <span className="text-sm font-mono">v{version}</span>
        </SettingContainer>

        <ShowWhatsNewOnUpdate descriptionMode="tooltip" grouped={true} />

        <SettingContainer
          title={t("settings.about.sourceCode.title")}
          description={t("settings.about.sourceCode.description")}
          grouped={true}
        >
          <Button
            variant="secondary"
            size="md"
            onClick={() => openUrl("https://github.com/Asmadey/popizdim-app")}
          >
            {t("settings.about.sourceCode.button")}
          </Button>
        </SettingContainer>

        <AppDataDirectory descriptionMode="tooltip" grouped={true} />
        <LogDirectory grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.about.author.title")}>
        <div className="flex flex-col gap-3 px-5 py-3">
          <h3 className="text-sm font-medium">
            {t("settings.about.author.name")}
          </h3>
          {AUTHOR_LINKS.map((link) => (
            <div key={link.url} className="flex flex-wrap items-center gap-2">
              <span className="text-sm text-mid-gray">{t(link.labelKey)}:</span>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => openUrl(link.url)}
              >
                {link.text}
              </Button>
            </div>
          ))}
        </div>
      </SettingsGroup>
    </div>
  );
};
