import React, { useRef, useState } from "react";
import { SettingContainer } from "./SettingContainer";
import { useShowEndOfText } from "../../hooks/useShowEndOfText";

interface TextDisplayProps {
  label: string;
  description: string;
  value: string;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  placeholder?: string;
  copyable?: boolean;
  monospace?: boolean;
  onCopy?: (value: string) => void;
}

export const TextDisplay: React.FC<TextDisplayProps> = ({
  label,
  description,
  value,
  descriptionMode = "tooltip",
  grouped = false,
  placeholder = "Not available",
  copyable = false,
  monospace = false,
  onCopy,
}) => {
  const [showCopied, setShowCopied] = useState(false);
  const textRef = useRef<HTMLDivElement>(null);
  useShowEndOfText(textRef, value);

  const handleCopy = async () => {
    if (!value || !copyable) return;

    try {
      await navigator.clipboard.writeText(value);
      setShowCopied(true);
      setTimeout(() => setShowCopied(false), 1500);
      if (onCopy) {
        onCopy(value);
      }
    } catch (err) {
      console.error("Failed to copy to clipboard:", err);
    }
  };

  const displayValue = value || placeholder;
  const textClasses = monospace ? "font-mono" : "";

  return (
    <SettingContainer
      title={label}
      description={description}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex items-center space-x-2">
        <div className="flex-1 min-w-0">
          <div
            ref={textRef}
            title={value}
            className={`px-3 min-h-8 flex items-center whitespace-nowrap overflow-x-hidden bg-[var(--field-bg)] shadow-[0_0_0_1px_var(--field-ring)] rounded-xl text-xs ${textClasses} ${!value ? "opacity-60" : ""}`}
          >
            <span className="whitespace-nowrap">{displayValue}</span>
          </div>
        </div>
        {copyable && value && (
          <button
            onClick={handleCopy}
            className="flex items-center justify-center px-2 py-1 w-12 min-h-8 text-xs font-semibold bg-mid-gray/10 border border-mid-gray/80 hover:shadow-[0_0_0_1px_var(--color-logo-primary)] hover:text-logo-primary rounded-xl transition-all duration-150 flex-shrink-0 cursor-pointer"
            title="Copy to clipboard"
          >
            {showCopied ? (
              <div className="flex items-center space-x-1">
                <svg
                  className="w-4 h-4"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M5 13l4 4L19 7"
                  />
                </svg>
              </div>
            ) : (
              "Copy"
            )}
          </button>
        )}
      </div>
    </SettingContainer>
  );
};
