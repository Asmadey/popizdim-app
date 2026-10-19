import { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  requestAccessibilityPermission,
  checkMicrophonePermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { useSettingsStore } from "@/stores/settingsStore";
import OnboardingNav from "./OnboardingNav";
import { ONBOARDING_STEP_COUNT } from "./WelcomeTour";
import { useModelStore } from "@/stores/modelStore";
import {
  Keyboard,
  Languages,
  Mic,
  Check,
  Loader2,
  type LucideIcon,
} from "lucide-react";

// Neutral glass buttons: the accent color is kept for small highlights, a
// full-size blue button looked cheap next to the rest of the dark screen.
const ACTION_BUTTON =
  "rounded-lg border border-text/[0.07] bg-text/[0.09] px-3.5 py-1.5 text-sm font-medium text-text shadow-[inset_0_1px_0_rgb(255_255_255/0.06)] transition-all hover:bg-text/[0.14] active:scale-[0.98] cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-text/30";
const STATUS_PILL =
  "flex w-fit items-center gap-1.5 rounded-lg bg-text/[0.04] px-3 py-1.5 text-sm text-text/45";

// The two presets offered on the last onboarding step.
const MODEL_RUSSIAN = "gigaam-v3-e2e-ctc";
const MODEL_ENGLISH = "parakeet-tdt-0.6b-v2";
const MODEL_PRESETS = [
  { id: MODEL_RUSSIAN, flag: "🇷🇺", labelKey: "russian" },
  { id: MODEL_ENGLISH, flag: "🇺🇸", labelKey: "english" },
] as const;

interface AccessibilityOnboardingProps {
  /** `modelSelected` is true when the model preset row was used. */
  onComplete: (modelSelected?: boolean) => void;
  /** Adds a row that picks and downloads the transcription model. */
  withModelChoice?: boolean;
  /** Back to the last welcome slide; omitted when there is no tour before. */
  onBack?: () => void;
  preview?: boolean;
}

type PermissionStatus = "checking" | "needed" | "waiting" | "granted";
type PermissionPlatform = "macos" | "windows" | "other";

interface PermissionsState {
  accessibility: PermissionStatus;
  microphone: PermissionStatus;
}

const AccessibilityOnboarding: React.FC<AccessibilityOnboardingProps> = ({
  onComplete,
  withModelChoice = false,
  onBack,
  preview = false,
}) => {
  const { t } = useTranslation();
  const refreshAudioDevices = useSettingsStore(
    (state) => state.refreshAudioDevices,
  );
  const refreshOutputDevices = useSettingsStore(
    (state) => state.refreshOutputDevices,
  );
  const [permissionPlatform, setPermissionPlatform] =
    useState<PermissionPlatform | null>(null);
  const [permissions, setPermissions] = useState<PermissionsState>({
    accessibility: "checking",
    microphone: "checking",
  });
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const errorCountRef = useRef<number>(0);
  const MAX_POLLING_ERRORS = 3;

  const isMacOS = permissionPlatform === "macos";
  const isWindows = permissionPlatform === "windows";
  const showMicrophonePermission = isMacOS || isWindows;
  const showAccessibilityPermission = isMacOS;

  const permissionsGranted = isMacOS
    ? permissions.accessibility === "granted" &&
      permissions.microphone === "granted"
    : isWindows
      ? permissions.microphone === "granted"
      : true;

  // Model preset row: picking a language downloads its model (or reuses one
  // already on disk) and selects it, the same way the model list does.
  const {
    models,
    downloadModel,
    selectModel,
    downloadingModels,
    verifyingModels,
    extractingModels,
    downloadProgress,
  } = useModelStore();
  const [chosenModelId, setChosenModelId] = useState<string | null>(null);
  const [modelReady, setModelReady] = useState(false);
  const selectingModelRef = useRef(false);

  const allGranted = permissionsGranted && (!withModelChoice || modelReady);

  useEffect(() => {
    if (preview || !chosenModelId || selectingModelRef.current) return;

    const model = models.find((m) => m.id === chosenModelId);
    const busy =
      chosenModelId in downloadingModels ||
      chosenModelId in verifyingModels ||
      chosenModelId in extractingModels;
    if (!model?.is_downloaded || busy) return;

    selectingModelRef.current = true;
    selectModel(chosenModelId).then((success) => {
      if (success) {
        setModelReady(true);
      } else {
        toast.error(t("onboarding.errors.selectModel"));
        selectingModelRef.current = false;
        setChosenModelId(null);
      }
    });
  }, [
    chosenModelId,
    models,
    downloadingModels,
    verifyingModels,
    extractingModels,
    selectModel,
    preview,
    t,
  ]);

  const handleChooseModel = async (modelId: string) => {
    if (preview || chosenModelId) return;

    setChosenModelId(modelId);
    if (models.find((m) => m.id === modelId)?.is_downloaded) return;

    // The failure toast comes from the model store's download listener
    const started = await downloadModel(modelId);
    if (!started) setChosenModelId(null);
  };

  const completeOnboarding = useCallback(
    async (modelSelected: boolean) => {
      await Promise.all([refreshAudioDevices(), refreshOutputDevices()]);
      timeoutRef.current = setTimeout(() => onComplete(modelSelected), 300);
    },
    [onComplete, refreshAudioDevices, refreshOutputDevices],
  );

  // Finish once the permissions are in and, when asked for, the model is
  // selected. Kept in one place so a row finishing first never skips the rest.
  const completedRef = useRef(false);
  useEffect(() => {
    if (preview || completedRef.current) return;
    if (!(isMacOS || isWindows) || !allGranted) return;

    completedRef.current = true;
    completeOnboarding(withModelChoice);
  }, [
    allGranted,
    completeOnboarding,
    isMacOS,
    isWindows,
    preview,
    withModelChoice,
  ]);

  const hasWindowsMicrophoneAccess = useCallback(async (): Promise<boolean> => {
    const microphoneStatus =
      await commands.getWindowsMicrophonePermissionStatus();

    if (!microphoneStatus.supported) {
      return true;
    }

    return microphoneStatus.overall_access !== "denied";
  }, []);

  // Check platform and permission status on mount
  useEffect(() => {
    const currentPlatform = platform();
    const nextPlatform: PermissionPlatform =
      currentPlatform === "macos"
        ? "macos"
        : currentPlatform === "windows"
          ? "windows"
          : "other";

    setPermissionPlatform(nextPlatform);

    // Debug previews are intentionally inert: show the permission request UI
    // without checking or changing operating-system permissions.
    if (preview) {
      setPermissions({
        accessibility: nextPlatform === "macos" ? "needed" : "granted",
        microphone: nextPlatform === "other" ? "granted" : "needed",
      });
      return;
    }

    // Skip immediately on unsupported platforms
    if (nextPlatform === "other") {
      onComplete();
      return;
    }

    const checkInitial = async () => {
      if (nextPlatform === "macos") {
        try {
          const [accessibilityGranted, microphoneGranted] = await Promise.all([
            checkAccessibilityPermission(),
            checkMicrophonePermission(),
          ]);

          // If accessibility is granted, initialize Enigo and shortcuts
          if (accessibilityGranted) {
            try {
              await Promise.all([
                commands.initializeEnigo(),
                commands.initializeShortcuts(),
              ]);
            } catch (e) {
              console.warn("Failed to initialize after permission grant:", e);
            }
          }

          const newState: PermissionsState = {
            accessibility: accessibilityGranted ? "granted" : "needed",
            microphone: microphoneGranted ? "granted" : "needed",
          };

          setPermissions(newState);
        } catch (error) {
          console.error("Failed to check macOS permissions:", error);
          toast.error(t("onboarding.permissions.errors.checkFailed"));
          setPermissions({
            accessibility: "needed",
            microphone: "needed",
          });
        }

        return;
      }

      try {
        const microphoneGranted = await hasWindowsMicrophoneAccess();

        setPermissions({
          accessibility: "granted",
          microphone: microphoneGranted ? "granted" : "needed",
        });
      } catch (error) {
        console.warn("Failed to check Windows microphone permissions:", error);
        setPermissions({
          accessibility: "granted",
          microphone: "granted",
        });
      }
    };

    checkInitial();
  }, [hasWindowsMicrophoneAccess, onComplete, preview, t]);

  // Polling for permissions after user clicks a button
  const startPolling = useCallback(() => {
    if (pollingRef.current || permissionPlatform === null) return;

    pollingRef.current = setInterval(async () => {
      try {
        if (permissionPlatform === "windows") {
          const microphoneGranted = await hasWindowsMicrophoneAccess();

          if (microphoneGranted) {
            setPermissions((prev) => ({ ...prev, microphone: "granted" }));

            if (pollingRef.current) {
              clearInterval(pollingRef.current);
              pollingRef.current = null;
            }
          }

          errorCountRef.current = 0;
          return;
        }

        const [accessibilityGranted, microphoneGranted] = await Promise.all([
          checkAccessibilityPermission(),
          checkMicrophonePermission(),
        ]);

        setPermissions((prev) => {
          const newState = { ...prev };

          if (accessibilityGranted && prev.accessibility !== "granted") {
            newState.accessibility = "granted";
            // Initialize Enigo and shortcuts when accessibility is granted
            Promise.all([
              commands.initializeEnigo(),
              commands.initializeShortcuts(),
            ]).catch((e) => {
              console.warn("Failed to initialize after permission grant:", e);
            });
          }

          if (microphoneGranted && prev.microphone !== "granted") {
            newState.microphone = "granted";
          }

          return newState;
        });

        // If both granted, stop polling; the completion effect takes over
        if (accessibilityGranted && microphoneGranted) {
          if (pollingRef.current) {
            clearInterval(pollingRef.current);
            pollingRef.current = null;
          }
        }

        // Reset error count on success
        errorCountRef.current = 0;
      } catch (error) {
        console.error("Error checking permissions:", error);
        errorCountRef.current += 1;

        if (errorCountRef.current >= MAX_POLLING_ERRORS) {
          // Stop polling after too many consecutive errors
          if (pollingRef.current) {
            clearInterval(pollingRef.current);
            pollingRef.current = null;
          }
          toast.error(t("onboarding.permissions.errors.checkFailed"));
        }
      }
    }, 1000);
  }, [hasWindowsMicrophoneAccess, permissionPlatform, t]);

  // Keep checking while anything is missing, not only after a button click:
  // after a rebuild the app is already listed in System Settings, and people
  // flip the toggle there directly without coming back to press "Grant".
  const anyNeeded =
    permissions.accessibility === "needed" ||
    permissions.microphone === "needed";
  useEffect(() => {
    if (preview || !anyNeeded) return;
    startPolling();
  }, [anyNeeded, preview, startPolling]);

  // Cleanup polling and timeouts on unmount
  useEffect(() => {
    return () => {
      if (pollingRef.current) {
        clearInterval(pollingRef.current);
      }
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);

  const handleGrantAccessibility = async () => {
    if (preview) return;

    try {
      await requestAccessibilityPermission();
      setPermissions((prev) => ({ ...prev, accessibility: "waiting" }));
      startPolling();
    } catch (error) {
      console.error("Failed to request accessibility permission:", error);
      toast.error(t("onboarding.permissions.errors.requestFailed"));
    }
  };

  const handleGrantMicrophone = async () => {
    if (preview) return;

    try {
      if (isWindows) {
        await commands.openMicrophonePrivacySettings();
      } else {
        await requestMicrophonePermission();
      }

      setPermissions((prev) => ({ ...prev, microphone: "waiting" }));
      startPolling();
    } catch (error) {
      console.error("Failed to request microphone permission:", error);
      toast.error(t("onboarding.permissions.errors.requestFailed"));
    }
  };

  const isChecking =
    permissionPlatform === null ||
    (isMacOS &&
      permissions.accessibility === "checking" &&
      permissions.microphone === "checking") ||
    (isWindows && permissions.microphone === "checking");

  // Still checking platform/initial permissions
  if (isChecking) {
    return (
      <div className="h-screen w-full flex items-center justify-center">
        <Loader2 className="w-8 h-8 animate-spin text-text/50" />
      </div>
    );
  }

  // All permissions granted - show success briefly
  if (allGranted) {
    return (
      <div className="h-screen w-full flex flex-col items-center justify-center gap-4">
        <div className="p-4 rounded-full bg-emerald-500/15 ring-8 ring-emerald-500/10">
          <Check className="w-12 h-12 text-emerald-500" />
        </div>
        <p className="text-lg font-medium text-text">
          {t("onboarding.permissions.allGranted")}
        </p>
      </div>
    );
  }

  const rows: PermissionRowProps[] = [];
  if (showMicrophonePermission) {
    rows.push({
      id: "microphone",
      icon: Mic,
      title: t("onboarding.permissions.microphone.title"),
      description: t("onboarding.permissions.microphone.description"),
      status: permissions.microphone,
      onGrant: handleGrantMicrophone,
      grantLabel: isWindows
        ? t("accessibility.openSettings")
        : t("onboarding.permissions.grant"),
      grantedLabel: t("onboarding.permissions.granted"),
      waitingLabel: t("onboarding.permissions.waiting"),
    });
  }
  if (showAccessibilityPermission) {
    rows.push({
      id: "accessibility",
      icon: Keyboard,
      title: t("onboarding.permissions.accessibility.title"),
      description: t("onboarding.permissions.accessibility.description"),
      status: permissions.accessibility,
      onGrant: handleGrantAccessibility,
      grantLabel: t("onboarding.permissions.grant"),
      grantedLabel: t("onboarding.permissions.granted"),
      waitingLabel: t("onboarding.permissions.waiting"),
    });
  }

  if (withModelChoice) {
    const chosen = MODEL_PRESETS.find((p) => p.id === chosenModelId);
    const preparing =
      !!chosenModelId &&
      (chosenModelId in verifyingModels || chosenModelId in extractingModels);
    const percent = Math.round(
      chosenModelId ? (downloadProgress[chosenModelId]?.percentage ?? 0) : 0,
    );

    let control: React.ReactNode;
    if (chosen && modelReady) {
      control = (
        <div className={STATUS_PILL}>
          <Check className="h-4 w-4" />
          {chosen.flag} {t(`onboarding.permissions.model.${chosen.labelKey}`)}
        </div>
      );
    } else if (chosen) {
      control = (
        <div className={`${STATUS_PILL} gap-2`}>
          <Loader2 className="h-4 w-4 animate-spin" />
          {chosen.flag} {t(`onboarding.permissions.model.${chosen.labelKey}`)}
          {" · "}
          {preparing
            ? t("onboarding.permissions.model.preparing")
            : t("onboarding.permissions.model.downloading", { percent })}
        </div>
      );
    } else {
      control = (
        <div className="flex flex-wrap gap-2">
          {MODEL_PRESETS.map((preset) => (
            <button
              key={preset.id}
              onClick={() => handleChooseModel(preset.id)}
              className={`flex items-center gap-2 ${ACTION_BUTTON}`}
            >
              <span className="text-base leading-none">{preset.flag}</span>
              {t(`onboarding.permissions.model.${preset.labelKey}`)}
            </button>
          ))}
        </div>
      );
    }

    rows.push({
      id: "model",
      icon: Languages,
      title: t("onboarding.permissions.model.title"),
      description: t("onboarding.permissions.model.description"),
      control,
    });
  }

  // Show permissions request screen: the last step of the welcome tour
  return (
    <div className="relative h-screen w-full overflow-hidden select-none bg-background text-text">
      <div className="welcome-slide relative grid h-full grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
        <div className="flex flex-col justify-center ps-10 pe-4 pb-20">
          <span className="mb-4 w-fit rounded-full bg-logo-primary/15 px-3 py-1 text-sm font-medium text-logo-primary">
            {t("onboarding.permissions.tag")}
          </span>
          <h1 className="mb-4 text-4xl font-semibold leading-[1.1] tracking-tight">
            {t("onboarding.permissions.title")}
          </h1>
          <p className="text-base leading-relaxed text-text/55">
            {t("onboarding.permissions.description")}
          </p>
        </div>

        <div className="flex flex-col justify-center divide-y divide-text/10 pe-10 pb-20 ps-2">
          {rows.map((row) => (
            <PermissionRow key={row.id} {...row} />
          ))}
        </div>
      </div>

      {onBack && (
        <div className="absolute inset-x-0 bottom-0 p-5">
          <OnboardingNav
            total={ONBOARDING_STEP_COUNT}
            current={ONBOARDING_STEP_COUNT - 1}
            onBack={onBack}
          />
        </div>
      )}
    </div>
  );
};

// Permission icons are picked up by file name (permission-<id>.webp), so a new
// picture needs no code; until it exists a plain tile with a glyph is shown.
const iconModules = import.meta.glob<string>(
  "../../assets/onboarding/permission-*.{webp,png,jpg,jpeg}",
  { eager: true, import: "default" },
);
const PERMISSION_ICONS: Record<string, string> = Object.fromEntries(
  Object.entries(iconModules).map(([path, url]) => [
    path
      .split("/")
      .pop()!
      .replace(/^permission-/, "")
      .replace(/\.[^.]+$/, ""),
    url,
  ]),
);

interface PermissionRowProps {
  id: string;
  icon: LucideIcon;
  title: string;
  description: string;
  /** Replaces the status / grant button, for rows that are not a plain permission. */
  control?: React.ReactNode;
  status?: PermissionStatus;
  onGrant?: () => void;
  grantLabel?: string;
  grantedLabel?: string;
  waitingLabel?: string;
}

const PermissionRow: React.FC<PermissionRowProps> = ({
  id,
  icon: Icon,
  title,
  description,
  control,
  status,
  onGrant,
  grantLabel,
  grantedLabel,
  waitingLabel,
}) => {
  const image = PERMISSION_ICONS[id];

  return (
    <div className="flex items-start gap-4 py-5 first:pt-0 last:pb-0">
      {/* bg-background gives `lighten` something to blend the picture with,
          so the picture's own background disappears into the page */}
      <div className="flex h-[72px] w-[72px] shrink-0 items-center justify-center bg-background">
        {image ? (
          <img
            src={image}
            alt=""
            draggable={false}
            style={{ mixBlendMode: "lighten" }}
            className="h-full w-full object-contain"
          />
        ) : (
          <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-text/5 ring-1 ring-inset ring-text/10">
            <Icon className="h-6 w-6 text-logo-primary" strokeWidth={1.5} />
          </div>
        )}
      </div>

      <div className="min-w-0 flex-1">
        <h3 className="font-medium leading-tight">{title}</h3>
        <p className="mt-1 text-sm leading-snug text-text/55">{description}</p>

        <div className="mt-3">
          {control ? (
            control
          ) : status === "granted" ? (
            <div className={STATUS_PILL}>
              <Check className="h-4 w-4" />
              {grantedLabel}
            </div>
          ) : status === "waiting" ? (
            <div className={`${STATUS_PILL} gap-2`}>
              <Loader2 className="h-4 w-4 animate-spin" />
              {waitingLabel}
            </div>
          ) : (
            <button onClick={onGrant} className={ACTION_BUTTON}>
              {grantLabel}
            </button>
          )}
        </div>
      </div>
    </div>
  );
};

export default AccessibilityOnboarding;
