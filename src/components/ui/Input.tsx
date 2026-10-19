import React, { useRef } from "react";
import { useShowEndOfText } from "../../hooks/useShowEndOfText";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  variant?: "default" | "compact";
}

export const Input: React.FC<InputProps> = ({
  className = "",
  variant = "default",
  disabled,
  ...props
}) => {
  const ref = useRef<HTMLInputElement>(null);
  useShowEndOfText(ref, props.value);

  const baseClasses =
    "px-2 py-1 text-sm font-medium bg-[var(--field-bg)] shadow-[0_0_0_1px_var(--field-ring)] rounded-xl text-start transition-all duration-150";

  const interactiveClasses = disabled
    ? "opacity-60 cursor-not-allowed"
    : "hover:shadow-[0_0_0_1px_var(--color-logo-primary)] focus:outline-none focus:shadow-[0_0_0_2px_var(--color-logo-primary)]";

  const variantClasses = {
    default: "px-3 py-[7px]",
    compact: "px-2 py-1",
  } as const;

  return (
    <input
      ref={ref}
      className={`${baseClasses} ${variantClasses[variant]} ${interactiveClasses} ${className}`}
      disabled={disabled}
      {...props}
    />
  );
};
