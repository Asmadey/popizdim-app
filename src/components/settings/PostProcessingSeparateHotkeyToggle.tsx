import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface PostProcessingSeparateHotkeyToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** Gives post-processing a shortcut of its own instead of the regular one. */
export const PostProcessingSeparateHotkeyToggle: React.FC<PostProcessingSeparateHotkeyToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const separate = getSetting("post_process_separate_hotkey") || false;

    return (
      <ToggleSwitch
        checked={separate}
        onChange={(value) =>
          updateSetting("post_process_separate_hotkey", value)
        }
        isUpdating={isUpdating("post_process_separate_hotkey")}
        label={t("settings.postProcessing.hotkey.separate.label")}
        description={t("settings.postProcessing.hotkey.separate.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });
