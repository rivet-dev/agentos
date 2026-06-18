import type { ReactNode } from "react";

interface LiquidChromeButtonProps {
  children: ReactNode;
  href?: string;
  onClick?: () => void;
  className?: string;
}

export function LiquidChromeButton({ children, href, onClick, className = "" }: LiquidChromeButtonProps) {
  const inner = (
    <>
      <span className="chrome-layer chrome-glow" />
      <span className="chrome-layer chrome-shadow" />
      <span className="chrome-layer chrome-base" />
      <span className="chrome-layer chrome-rim" />
      <span className="chrome-layer chrome-bevel" />
      <span className="chrome-layer chrome-top" />
      <span className="chrome-layer chrome-bottom" />
      <span className="chrome-layer chrome-sides" />
      <span className="chrome-layer chrome-sparks" />
      <span className="chrome-layer chrome-sheen" />
      <span className="chrome-label">{children}</span>
    </>
  );

  if (href) {
    return (
      <a href={href} className={`chrome-btn ${className}`}>
        {inner}
      </a>
    );
  }

  return (
    <button onClick={onClick} className={`chrome-btn ${className}`}>
      {inner}
    </button>
  );
}
