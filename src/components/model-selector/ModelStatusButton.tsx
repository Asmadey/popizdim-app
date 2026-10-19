import React from "react";

type ModelStatus =
  | "ready"
  | "loading"
  | "downloading"
  | "verifying"
  | "extracting"
  | "error"
  | "unloaded"
  | "none";

interface ModelStatusButtonProps {
  status: ModelStatus;
  displayText: string;
  isDropdownOpen: boolean;
  onClick: () => void;
  className?: string;
  // "panel": full-width pill that matches the sidebar navigation items
  variant?: "inline" | "panel";
}

const ModelStatusButton: React.FC<ModelStatusButtonProps> = ({
  status,
  displayText,
  isDropdownOpen,
  onClick,
  className = "",
  variant = "inline",
}) => {
  const getStatusColor = (status: ModelStatus): string => {
    switch (status) {
      case "ready":
        return "bg-green-400";
      case "loading":
        return "bg-yellow-400 animate-pulse";
      case "downloading":
        return "bg-logo-primary animate-pulse";
      case "verifying":
        return "bg-orange-400 animate-pulse";
      case "extracting":
        return "bg-orange-400 animate-pulse";
      case "error":
        return "bg-red-400";
      case "unloaded":
        return "bg-mid-gray/60";
      case "none":
        return "bg-red-400";
      default:
        return "bg-mid-gray/60";
    }
  };

  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-2 transition-colors ${
        variant === "panel"
          ? "w-full rounded-[10px] bg-text/5 px-2 py-1.5 text-sm text-text hover:bg-text/10 cursor-pointer"
          : "hover:text-text/80"
      } ${className}`}
      title={`Model status: ${displayText}`}
    >
      <div
        className={`w-2 h-2 shrink-0 rounded-full ${getStatusColor(status)}`}
      />
      <span
        className={
          variant === "panel"
            ? "min-w-0 flex-1 truncate text-start"
            : "max-w-28 truncate"
        }
      >
        {displayText}
      </span>
      <svg
        className={`w-3 h-3 shrink-0 transition-transform ${isDropdownOpen ? "rotate-180" : ""}`}
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M19 9l-7 7-7-7"
        />
      </svg>
    </button>
  );
};

export default ModelStatusButton;
