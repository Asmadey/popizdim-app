import React from "react";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?:
    | "primary"
    | "primary-soft"
    | "secondary"
    | "warning"
    | "danger"
    | "danger-ghost"
    | "ghost";
  size?: "sm" | "md" | "lg";
}

export const Button: React.FC<ButtonProps> = ({
  children,
  className = "",
  variant = "primary",
  size = "md",
  ...props
}) => {
  const baseClasses =
    "font-medium rounded-xl border focus:outline-none transition-colors disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer";

  const variantClasses = {
    primary:
      "text-white bg-background-ui border-transparent shadow-[0_1px_2px_#0000002e] hover:brightness-110 focus-visible:ring-2 focus-visible:ring-background-ui/50",
    "primary-soft":
      "text-text bg-logo-primary/20 border-transparent hover:bg-logo-primary/30 focus:ring-1 focus:ring-logo-primary",
    secondary:
      "bg-mid-gray/15 border-transparent hover:bg-mid-gray/25 focus-visible:ring-2 focus-visible:ring-logo-primary/50",
    // Secondary's neutral resting look, but hover/focus use the semantic
    // --color-warning token (theme.css) instead of the pink accent — for
    // buttons sitting on warning surfaces like SecureInputWarning
    warning:
      "text-text bg-mid-gray/15 border-transparent hover:bg-warning/15 focus-visible:ring-2 focus-visible:ring-warning/50",
    danger:
      "text-white bg-red-600 border-mid-gray/20 hover:bg-red-700 hover:border-red-700 focus:ring-1 focus:ring-red-500",
    "danger-ghost":
      "text-red-400 border-transparent hover:text-red-300 hover:bg-red-500/10 focus:bg-red-500/20",
    ghost:
      "text-current border-transparent hover:bg-mid-gray/15 focus:bg-mid-gray/20",
  };

  const sizeClasses = {
    sm: "px-3 py-1 text-xs",
    md: "px-4 py-[6px] text-sm",
    lg: "px-4 py-2 text-base",
  };

  return (
    <button
      className={`${baseClasses} ${variantClasses[variant]} ${sizeClasses[size]} ${className}`}
      {...props}
    >
      {children}
    </button>
  );
};
