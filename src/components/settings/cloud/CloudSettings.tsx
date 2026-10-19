import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type CloudSettings as CloudConfig } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { useModelStore } from "../../../stores/modelStore";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { Dropdown } from "../../ui/Dropdown";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";

const AZURE_MODEL_ID = "cloud/azure/mai-transcribe-2-streaming";
const OPENROUTER_MODEL_ID = "cloud/openrouter/mai-transcribe-2";

type CheckState = {
  status: "idle" | "running" | "ok" | "error";
  message?: string;
};

/** Text field that saves on blur, like the post-processing API key field. */
const SavedField: React.FC<{
  value: string;
  onSave: (value: string) => void;
  secret?: boolean;
  placeholder?: string;
}> = ({ value, onSave, secret = false, placeholder }) => {
  const [local, setLocal] = useState(value);
  useEffect(() => setLocal(value), [value]);
  return (
    <Input
      type={secret ? "password" : "text"}
      value={local}
      onChange={(event) => setLocal(event.target.value)}
      onBlur={() => local !== value && onSave(local.trim())}
      placeholder={placeholder}
      variant="compact"
      spellCheck={false}
      autoComplete="off"
      className="flex-1 min-w-[320px]"
    />
  );
};

/** Backend errors for missing or malformed credentials, keyed by message prefix. */
const CONFIG_ERRORS: [prefix: string, key: string][] = [
  ["Azure endpoint is not set", "settings.cloud.errors.azureEndpointMissing"],
  ["Azure API key is not set", "settings.cloud.errors.azureKeyMissing"],
  ["Azure endpoint must start", "settings.cloud.errors.azureEndpointScheme"],
  ["Azure endpoint has no host", "settings.cloud.errors.azureEndpointHost"],
  [
    "OpenRouter API key is not set",
    "settings.cloud.errors.openrouterKeyMissing",
  ],
];

const CheckResult: React.FC<{ state: CheckState }> = ({ state }) => {
  const { t } = useTranslation();
  if (state.status === "idle" || state.status === "running") return null;
  const known = CONFIG_ERRORS.find(([prefix]) =>
    state.message?.startsWith(prefix),
  );
  const message = known ? t(known[1]) : state.message;
  return (
    <p
      className={`px-4 pb-2 text-xs ${
        state.status === "ok" ? "text-green-500" : "text-red-400"
      }`}
    >
      {message}
    </p>
  );
};

export const CloudSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateSetting } = useSettings();
  const currentModel = useModelStore((s) => s.currentModel);
  const selectModel = useModelStore((s) => s.selectModel);
  const [checks, setChecks] = useState<Record<string, CheckState>>({});

  const cloud = settings?.cloud;
  if (!cloud) return null;

  const save = (patch: Partial<CloudConfig>) =>
    updateSetting("cloud", { ...cloud, ...patch });

  const runCheck = async (provider: "openrouter" | "azure") => {
    setChecks((c) => ({ ...c, [provider]: { status: "running" } }));
    const result = await commands.testCloudProvider(provider);
    setChecks((c) => ({
      ...c,
      [provider]:
        result.status === "ok"
          ? { status: "ok", message: result.data }
          : { status: "error", message: result.error },
    }));
  };

  const providerButton = (modelId: string) =>
    currentModel === modelId ? (
      <span className="text-xs font-medium text-logo-primary">
        {t("settings.cloud.active")}
      </span>
    ) : (
      <Button
        variant="secondary"
        size="sm"
        onClick={() => selectModel(modelId)}
      >
        {t("settings.cloud.useProvider")}
      </Button>
    );

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup
        title={t("settings.cloud.azure.title")}
        description={t("settings.cloud.azure.description")}
      >
        <SettingContainer
          title={t("settings.cloud.provider")}
          description={t("settings.cloud.azure.providerDescription")}
          grouped
        >
          {providerButton(AZURE_MODEL_ID)}
        </SettingContainer>
        <SettingContainer
          title={t("settings.cloud.azure.endpoint")}
          description={t("settings.cloud.azure.endpointDescription")}
          layout="stacked"
          grouped
        >
          <SavedField
            value={cloud.azure_endpoint}
            onSave={(azure_endpoint) => save({ azure_endpoint })}
            placeholder="https://my-resource.services.ai.azure.com"
          />
        </SettingContainer>
        <SettingContainer
          title={t("settings.cloud.azure.apiKey")}
          description={t("settings.cloud.azure.apiKeyDescription")}
          layout="stacked"
          grouped
        >
          <SavedField
            secret
            value={cloud.azure_api_key}
            onSave={(azure_api_key) => save({ azure_api_key })}
            placeholder="AZURE_MAI_API_KEY"
          />
        </SettingContainer>
        <SettingContainer
          title={t("settings.cloud.azure.deployment")}
          description={t("settings.cloud.azure.deploymentDescription")}
          layout="stacked"
          grouped
        >
          <SavedField
            value={cloud.azure_deployment}
            onSave={(azure_deployment) => save({ azure_deployment })}
            placeholder="MAI-Transcribe-2-Streaming"
          />
        </SettingContainer>
        <ToggleSwitch
          checked={cloud.fallback_to_openrouter}
          onChange={(fallback_to_openrouter) =>
            save({ fallback_to_openrouter })
          }
          label={t("settings.cloud.azure.fallback")}
          description={t("settings.cloud.azure.fallbackDescription")}
          grouped
        />
        <SettingContainer
          title={t("settings.cloud.check")}
          description={t("settings.cloud.checkDescription")}
          grouped
        >
          <Button
            variant="secondary"
            size="sm"
            disabled={checks.azure?.status === "running"}
            onClick={() => runCheck("azure")}
          >
            {checks.azure?.status === "running"
              ? t("settings.cloud.checking")
              : t("settings.cloud.checkButton")}
          </Button>
        </SettingContainer>
        <CheckResult state={checks.azure ?? { status: "idle" }} />
      </SettingsGroup>

      <SettingsGroup
        title={t("settings.cloud.openrouter.title")}
        description={t("settings.cloud.openrouter.description")}
      >
        <SettingContainer
          title={t("settings.cloud.provider")}
          description={t("settings.cloud.openrouter.providerDescription")}
          grouped
        >
          {providerButton(OPENROUTER_MODEL_ID)}
        </SettingContainer>
        <SettingContainer
          title={t("settings.cloud.openrouter.apiKey")}
          description={t("settings.cloud.openrouter.apiKeyDescription")}
          layout="stacked"
          grouped
        >
          <SavedField
            secret
            value={cloud.openrouter_api_key}
            onSave={(openrouter_api_key) => save({ openrouter_api_key })}
            placeholder="sk-or-v1-…"
          />
        </SettingContainer>
        <SettingContainer
          title={t("settings.cloud.openrouter.style")}
          description={t("settings.cloud.openrouter.styleDescription")}
          grouped
        >
          <Dropdown
            selectedValue={cloud.transcribe_style}
            onSelect={(transcribe_style) => save({ transcribe_style })}
            options={[
              {
                value: "verbatim",
                label: t("settings.cloud.openrouter.styleVerbatim"),
              },
              {
                value: "clean",
                label: t("settings.cloud.openrouter.styleClean"),
              },
            ]}
          />
        </SettingContainer>
        <SettingContainer
          title={t("settings.cloud.check")}
          description={t("settings.cloud.checkDescription")}
          grouped
        >
          <Button
            variant="secondary"
            size="sm"
            disabled={checks.openrouter?.status === "running"}
            onClick={() => runCheck("openrouter")}
          >
            {checks.openrouter?.status === "running"
              ? t("settings.cloud.checking")
              : t("settings.cloud.checkButton")}
          </Button>
        </SettingContainer>
        <CheckResult state={checks.openrouter ?? { status: "idle" }} />
      </SettingsGroup>
    </div>
  );
};
