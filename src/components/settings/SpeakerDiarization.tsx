import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, type DiarizationModelStatus } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { SettingContainer } from "../ui/SettingContainer";
import { Button } from "../ui/Button";

interface SpeakerDiarizationProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

type DownloadProgress = { downloaded: number; total: number };

/** "Speaker labels in files" toggle plus the diarization model's state. */
export const SpeakerDiarization: React.FC<SpeakerDiarizationProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const enabled = getSetting("diarization_enabled") ?? true;
  const [status, setStatus] = useState<DiarizationModelStatus | null>(null);
  const [percent, setPercent] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const result = await commands.getDiarizationModelStatus();
    if (result.status === "ok") {
      setStatus(result.data);
      if (!result.data.downloading) setPercent(null);
    }
  }, []);

  useEffect(() => {
    refresh();
    // The tray can download the model too; follow it from here.
    const unlisten = listen<DownloadProgress>(
      "diarization-model-progress",
      ({ payload }) => {
        const done = payload.total > 0 && payload.downloaded >= payload.total;
        setPercent(
          done ? null : Math.floor((payload.downloaded / payload.total) * 100),
        );
        if (done) refresh();
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refresh]);

  const download = async () => {
    setError(null);
    setPercent(0);
    const result = await commands.downloadDiarizationModel();
    if (result.status === "error") setError(result.error);
    await refresh();
  };

  const remove = async () => {
    setError(null);
    const result = await commands.deleteDiarizationModel();
    if (result.status === "error") setError(result.error);
    await refresh();
  };

  const size = status ? Math.round(status.size_bytes / 1e6) : 273;
  const downloading = percent !== null || status?.downloading;

  return (
    <>
      <ToggleSwitch
        checked={enabled}
        onChange={(next) => updateSetting("diarization_enabled", next)}
        isUpdating={isUpdating("diarization_enabled")}
        disabled={status?.supported === false}
        label={t("settings.advanced.diarization.title")}
        description={t("settings.advanced.diarization.description", { size })}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
      {status && enabled && (
        <SettingContainer
          title={
            !status.supported
              ? t("settings.advanced.diarization.unsupported")
              : status.ready
                ? t("settings.advanced.diarization.ready")
                : t("settings.advanced.diarization.missing")
          }
          description={t("settings.advanced.diarization.description", {
            size,
          })}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          {status.supported &&
            (downloading ? (
              <span className="text-xs text-mid-gray">
                {t("settings.advanced.diarization.downloading", {
                  percent: percent ?? 0,
                })}
              </span>
            ) : status.ready ? (
              <Button variant="secondary" size="sm" onClick={remove}>
                {t("settings.advanced.diarization.delete")}
              </Button>
            ) : (
              <Button variant="secondary" size="sm" onClick={download}>
                {t("settings.advanced.diarization.download")}
              </Button>
            ))}
        </SettingContainer>
      )}
      {error && (
        <p className="px-4 pb-2 text-xs text-red-400">
          {t("settings.advanced.diarization.failed", { error })}
        </p>
      )}
    </>
  );
};
