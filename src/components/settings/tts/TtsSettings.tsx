import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, type CloudSettings as CloudConfig } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown } from "../../ui/Dropdown";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { Alert } from "../../ui/Alert";
import { PathDisplay } from "../../ui/PathDisplay";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { ShortcutInput } from "../ShortcutInput";

const TTS_MODELS = [
  { value: "microsoft/mai-voice-2.1-flash", label: "MAI-Voice-2.1-Flash" },
  { value: "microsoft/mai-voice-2.1", label: "MAI-Voice-2.1" },
];

/** MAI-Voice stops speeding up near 2×. */
const TTS_SPEEDS = [1, 1.25, 1.5, 1.75, 2].map((speed) => ({
  value: String(speed),
  label: `${speed}×`,
}));

/**
 * Text-to-speech: voice, model, speed, where spoken audio is saved, the
 * "Speak Text" shortcut and a test field.
 */
export const TtsSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateSetting } = useSettings();
  const [voices, setVoices] = useState<string[]>([]);
  const [sample, setSample] = useState("");
  const [speaking, setSpeaking] = useState(false);
  const [speakError, setSpeakError] = useState<string | null>(null);
  const [saveDir, setSaveDir] = useState("");

  useEffect(() => {
    commands.getTtsVoices().then(setVoices);
  }, []);

  const cloud = settings?.cloud;
  const customSaveDir = cloud?.tts_save_dir ?? "";

  // The effective folder (custom or the app-data default), refreshed on change.
  useEffect(() => {
    commands.getTtsSaveDir().then((result) => {
      if (result.status === "ok") setSaveDir(result.data);
    });
  }, [customSaveDir]);
  if (!cloud) return null;

  // TTS settings live in the cloud settings object next to the OpenRouter key.
  const save = (patch: Partial<CloudConfig>) =>
    updateSetting("cloud", { ...cloud, ...patch });

  const chooseSaveDir = async () => {
    const picked = await open({
      directory: true,
      defaultPath: saveDir || undefined,
      title: t("settings.tts.saveDirPick"),
    });
    if (typeof picked === "string") save({ tts_save_dir: picked });
  };

  const speak = async () => {
    if (speaking) {
      await commands.stopSpeaking();
      return;
    }
    setSpeaking(true);
    setSpeakError(null);
    const result = await commands.speakText(
      sample.trim() || t("settings.tts.sampleDefault"),
    );
    if (result.status === "error") setSpeakError(result.error);
    setSpeaking(false);
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <p className="px-4 text-sm text-mid-gray">{t("settings.tts.intro")}</p>
      {!cloud.openrouter_api_key && (
        <Alert variant="info">{t("settings.tts.keyMissing")}</Alert>
      )}

      <SettingsGroup
        title={t("settings.tts.voiceGroup")}
        description={t("settings.tts.voiceGroupDescription")}
      >
        <SettingContainer
          title={t("settings.tts.model")}
          description={t("settings.tts.modelDescription")}
          grouped
        >
          <Dropdown
            selectedValue={cloud.tts_model}
            onSelect={(tts_model) => save({ tts_model })}
            options={TTS_MODELS}
          />
        </SettingContainer>
        <SettingContainer
          title={t("settings.tts.voice")}
          description={t("settings.tts.voiceDescription")}
          grouped
        >
          <Dropdown
            selectedValue={cloud.tts_voice}
            onSelect={(tts_voice) => save({ tts_voice })}
            options={voices.map((v) => ({ value: v, label: v }))}
            menuClassName="max-h-72 overflow-y-auto"
          />
        </SettingContainer>
        <SettingContainer
          title={t("settings.tts.speed")}
          description={t("settings.tts.speedDescription")}
          grouped
        >
          <Dropdown
            selectedValue={String(cloud.tts_speed)}
            onSelect={(speed) => save({ tts_speed: Number(speed) })}
            options={TTS_SPEEDS}
          />
        </SettingContainer>
        <SettingContainer
          title={t("settings.tts.try")}
          description={t("settings.tts.tryDescription")}
          layout="stacked"
          grouped
        >
          <div className="flex gap-2">
            <Input
              value={sample}
              onChange={(event) => setSample(event.target.value)}
              placeholder={t("settings.tts.sampleDefault")}
              variant="compact"
              className="flex-1"
            />
            <Button variant="secondary" size="sm" onClick={speak}>
              {speaking ? t("settings.tts.stop") : t("settings.tts.speak")}
            </Button>
          </div>
        </SettingContainer>
        {speakError && (
          <p className="px-4 pb-2 text-xs text-red-400">{speakError}</p>
        )}
      </SettingsGroup>

      <SettingsGroup
        title={t("settings.tts.saveGroup")}
        description={t("settings.tts.saveGroupDescription")}
      >
        <ToggleSwitch
          checked={cloud.tts_save_audio}
          onChange={(tts_save_audio) => save({ tts_save_audio })}
          label={t("settings.tts.saveAudio")}
          description={t("settings.tts.saveAudioDescription")}
          descriptionMode="inline"
          grouped
        />
        <SettingContainer
          title={t("settings.tts.saveDir")}
          description={t("settings.tts.saveDirDescription")}
          layout="stacked"
          grouped
        >
          <div className="space-y-2">
            <PathDisplay
              path={saveDir}
              onOpen={() => commands.openTtsSaveDir()}
              disabled={!saveDir}
            />
            <div className="flex gap-2">
              <Button variant="secondary" size="sm" onClick={chooseSaveDir}>
                {t("settings.tts.saveDirChange")}
              </Button>
              {customSaveDir && (
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => save({ tts_save_dir: "" })}
                >
                  {t("settings.tts.saveDirReset")}
                </Button>
              )}
            </div>
          </div>
        </SettingContainer>
      </SettingsGroup>

      <SettingsGroup
        title={t("settings.tts.shortcutGroup")}
        description={t("settings.tts.shortcutGroupDescription")}
      >
        <ShortcutInput shortcutId="speak_selection" grouped={true} />
        <p className="px-4 py-2 text-xs text-mid-gray">
          {t("settings.tts.selectionNote")}
        </p>
      </SettingsGroup>
    </div>
  );
};
