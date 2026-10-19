import React, { useRef } from "react";
import { useShowEndOfText } from "../../hooks/useShowEndOfText";
import { useTranslation } from "react-i18next";
import { Button } from "./Button";

interface PathDisplayProps {
  path: string;
  onOpen: () => void;
  disabled?: boolean;
}

export const PathDisplay: React.FC<PathDisplayProps> = ({
  path,
  onOpen,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const textRef = useRef<HTMLDivElement>(null);

  useShowEndOfText(textRef, path);

  return (
    <div className="flex items-center gap-2">
      <div
        ref={textRef}
        title={path}
        className="flex-1 min-w-0 px-3 py-2 bg-[var(--field-bg)] shadow-[0_0_0_1px_var(--field-ring)] rounded-xl text-xs font-mono whitespace-nowrap overflow-x-hidden select-text cursor-text"
      >
        {path}
      </div>
      <Button
        onClick={onOpen}
        variant="secondary"
        size="sm"
        disabled={disabled}
        className="px-3 py-2"
      >
        {t("common.open")}
      </Button>
    </div>
  );
};
