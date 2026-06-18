import type { ReactNode } from "react";

interface ChromeBorderProps {
  children: ReactNode;
  className?: string;
}

export function ChromeBorder({ children, className = "" }: ChromeBorderProps) {
  return (
    <div className={`chrome-frame ${className}`}>
      <span className="chrome-layer chrome-frame-glow" />
      <span className="chrome-layer chrome-frame-rim" />
      <span className="chrome-layer chrome-frame-bevel" />
      <span className="chrome-layer chrome-frame-top" />
      {children}
    </div>
  );
}
