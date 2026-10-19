import { useTranslation } from "react-i18next";
import { ChevronLeft } from "lucide-react";

interface OnboardingNavProps {
  /** Number of steps in the whole onboarding, shown as segments. */
  total: number;
  /** Zero-based index of the current step. */
  current: number;
  /** Shown as a round back button; omitted on the first step. */
  onBack?: () => void;
  /** Called when a segment is clicked; segments from `selectableCount` on stay inert. */
  onSelect?: (index: number) => void;
  selectableCount?: number;
}

/** Back button and step segments in the bottom-left corner of onboarding. */
const OnboardingNav: React.FC<OnboardingNavProps> = ({
  total,
  current,
  onBack,
  onSelect,
  selectableCount = total,
}) => {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-4">
      {onBack && (
        <button
          type="button"
          onClick={onBack}
          aria-label={t("onboarding.welcome.back")}
          className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full border border-text/10 bg-text/5 text-text/80 transition-colors hover:bg-text/10 cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary/50"
        >
          <ChevronLeft className="h-5 w-5" />
        </button>
      )}

      <div className="flex gap-1.5">
        {Array.from({ length: total }, (_, i) => {
          const segment = (
            <span
              className={`block h-1 rounded-full transition-all duration-300 ${
                i === current
                  ? "w-10 bg-logo-primary shadow-[0_0_10px] shadow-logo-primary/60"
                  : i < current
                    ? "w-5 bg-logo-primary/40"
                    : "w-5 bg-text/15 group-hover:bg-text/30"
              }`}
            />
          );

          return onSelect && i < selectableCount ? (
            <button
              key={i}
              type="button"
              onClick={() => onSelect(i)}
              aria-label={t("onboarding.welcome.goToSlide", { number: i + 1 })}
              aria-current={i === current}
              className="group py-2 cursor-pointer"
            >
              {segment}
            </button>
          ) : (
            <div key={i} className="py-2" aria-current={i === current}>
              {segment}
            </div>
          );
        })}
      </div>
    </div>
  );
};

export default OnboardingNav;
