import React from "react";

interface SettingsGroupProps {
  title?: string;
  description?: string;
  children: React.ReactNode;
}

export const SettingsGroup: React.FC<SettingsGroupProps> = ({
  title,
  description,
  children,
}) => {
  return (
    <div className="space-y-2">
      {title && (
        <div className="px-4 group-header">
          <h2 className="text-sm font-medium">{title}</h2>
          {description && (
            <p className="text-xs text-mid-gray mt-0.5">{description}</p>
          )}
        </div>
      )}
      <div className="glass-card overflow-visible">
        <div className="divide-y divide-[color:var(--glass-hairline)]">
          {children}
        </div>
      </div>
    </div>
  );
};
