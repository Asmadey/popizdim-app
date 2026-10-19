import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  BookText,
  Cpu,
  CornerDownLeft,
  FileAudio,
  Mic,
  Sparkles,
  Volume2,
  type LucideIcon,
} from "lucide-react";
import { AppMark } from "../icons/AppLogo";
import OnboardingNav from "./OnboardingNav";

interface WelcomeTourProps {
  onComplete: () => void;
  /** Open on the last slide, when coming back from the permissions step. */
  startAtEnd?: boolean;
}

interface Slide {
  /** Key under `onboarding.welcome.slides` in the translation files. */
  id: string;
  /** Shown as a placeholder while the slide has no illustration. */
  icon?: LucideIcon;
}

// To add a screen: append an entry here, add its `title`/`description` under
// `onboarding.welcome.slides.<id>` and its label under
// `onboarding.welcome.tags.<id>` in the translation files, and drop its
// illustration into src/assets/onboarding/ as <id>.ru.webp and <id>.en.webp
// (or .png / .jpg). Russian UI gets the Russian picture, every other language
// the English one; a plain <id>.webp serves any language when it has no text.
// The first slide is the centered welcome; the rest are text + illustration.
const SLIDES: Slide[] = [
  { id: "welcome" },
  { id: "dictation", icon: Mic },
  { id: "models", icon: Cpu },
  { id: "vocabulary", icon: BookText },
  { id: "polish", icon: Sparkles },
  { id: "speak", icon: Volume2 },
  { id: "files", icon: FileAudio },
];

/** Tour slides plus the permissions step that follows them. */
export const ONBOARDING_STEP_COUNT = SLIDES.length + 1;

// Illustrations are picked up by file name, so a new picture needs no code.
// Keys are the file stem: "dictation.ru", "dictation.en" or just "dictation".
const illustrationModules = import.meta.glob<string>(
  "../../assets/onboarding/*.{webp,png,jpg,jpeg}",
  { eager: true, import: "default" },
);
const ILLUSTRATIONS: Record<string, string> = Object.fromEntries(
  Object.entries(illustrationModules).map(([path, url]) => [
    path
      .split("/")
      .pop()!
      .replace(/\.[^.]+$/, ""),
    url,
  ]),
);

// Feathers every edge of an illustration into the page, so a slightly
// different background shade in a generated picture leaves no visible seam.
const EDGE_FADE =
  "linear-gradient(to right, transparent, #000 6%, #000 94%, transparent)";
const EDGE_FADE_VERTICAL =
  "linear-gradient(to bottom, transparent, #000 6%, #000 94%, transparent)";
// `lighten` also makes a generated background that is a shade darker than the
// page (they rarely match exactly) take the page color instead of a dark box.
const ILLUSTRATION_MASK: React.CSSProperties = {
  mixBlendMode: "lighten",
  maskImage: `${EDGE_FADE}, ${EDGE_FADE_VERTICAL}`,
  maskComposite: "intersect",
  WebkitMaskImage: `${EDGE_FADE}, ${EDGE_FADE_VERTICAL}`,
  WebkitMaskComposite: "source-in",
};

const WelcomeTour: React.FC<WelcomeTourProps> = ({
  onComplete,
  startAtEnd = false,
}) => {
  const { t, i18n } = useTranslation();
  const [index, setIndex] = useState(startAtEnd ? SLIDES.length - 1 : 0);
  const isFirst = index === 0;
  const isLast = index === SLIDES.length - 1;
  const slide = SLIDES[index];
  const Icon = slide.icon;
  const lang = i18n.language?.toLowerCase().startsWith("ru") ? "ru" : "en";
  const illustration =
    ILLUSTRATIONS[`${slide.id}.${lang}`] ?? ILLUSTRATIONS[slide.id];

  const next = useCallback(() => {
    if (isLast) {
      onComplete();
    } else {
      setIndex((i) => i + 1);
    }
  }, [isLast, onComplete]);

  const previous = useCallback(() => {
    setIndex((i) => Math.max(0, i - 1));
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Enter" || event.key === "ArrowRight") {
        event.preventDefault();
        next();
      } else if (event.key === "ArrowLeft") {
        event.preventDefault();
        previous();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [next, previous]);

  return (
    <div className="relative h-screen w-full overflow-hidden select-none bg-background text-text">
      {/* Brand glow, welcome slide only: the illustrations are drawn on the
          flat page color, so the other slides must not tint the background. */}
      {isFirst && (
        <div
          aria-hidden
          className="pointer-events-none absolute left-1/2 top-[38%] h-[34rem] w-[34rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-logo-primary/10 blur-3xl"
        />
      )}

      {/* Keyed so each slide remounts and replays the entrance animation */}
      <div key={slide.id} className="welcome-slide relative h-full">
        {isFirst ? (
          <div className="flex h-full flex-col items-center justify-center px-8 pb-16 text-center">
            <AppMark size={128} className="mb-10 drop-shadow-xl" />
            <h1 className="mb-4 max-w-lg text-4xl font-semibold tracking-tight">
              {t(`onboarding.welcome.slides.${slide.id}.title`)}
            </h1>
            <p className="max-w-lg text-lg leading-relaxed text-text/60">
              {t(`onboarding.welcome.slides.${slide.id}.description`)}
            </p>
          </div>
        ) : (
          <div className="grid h-full grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
            <div className="flex flex-col justify-center ps-10 pe-4 pb-20">
              <span className="mb-4 w-fit rounded-full bg-logo-primary/15 px-3 py-1 text-sm font-medium text-logo-primary">
                {t(`onboarding.welcome.tags.${slide.id}`)}
              </span>
              <h1 className="mb-4 text-4xl font-semibold leading-[1.1] tracking-tight">
                {t(`onboarding.welcome.slides.${slide.id}.title`)}
              </h1>
              <p className="text-base leading-relaxed text-text/55">
                {t(`onboarding.welcome.slides.${slide.id}.description`)}
              </p>
            </div>

            <div className="relative flex items-center justify-center bg-background pb-16 pt-2">
              {illustration ? (
                <img
                  src={illustration}
                  alt=""
                  draggable={false}
                  style={ILLUSTRATION_MASK}
                  className="h-full w-full object-cover"
                />
              ) : (
                Icon && (
                  <Icon className="h-20 w-20 text-text/15" strokeWidth={1} />
                )
              )}
            </div>
          </div>
        )}
      </div>

      <div className="absolute inset-x-0 bottom-0 flex items-center gap-4 p-5">
        <OnboardingNav
          total={ONBOARDING_STEP_COUNT}
          current={index}
          onBack={isFirst ? undefined : previous}
          onSelect={setIndex}
          selectableCount={SLIDES.length}
        />

        <button
          type="button"
          onClick={next}
          className="ms-auto flex items-center gap-3 rounded-full border border-text/10 bg-text/10 py-2.5 ps-6 pe-3 text-sm font-medium transition-all hover:bg-text/15 active:scale-[0.98] cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary/50"
        >
          {isFirst
            ? t("onboarding.welcome.start")
            : t("onboarding.welcome.next")}
          <span className="flex h-6 w-6 items-center justify-center rounded-md border border-text/20">
            <CornerDownLeft className="h-3.5 w-3.5" />
          </span>
        </button>
      </div>
    </div>
  );
};

export default WelcomeTour;
