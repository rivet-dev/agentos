import type { LucideIcon } from "lucide-react";

interface FeatureIconProps {
  icon: LucideIcon;
  color?: string;
  bgColor?: string;
  hoverBgColor?: string;
  glowShadow?: string;
}

export function FeatureIcon({
  icon: Icon,
  color = "text-accent",
  bgColor = "bg-accent/10",
  hoverBgColor = "group-hover:bg-accent/20",
  glowShadow = "group-hover:shadow-[0_0_15px_rgba(59,130,246,0.5)]",
}: FeatureIconProps) {
  return (
    <div className={`rounded ${bgColor} p-2 ${color} transition-all duration-500 ${hoverBgColor} ${glowShadow}`}>
      <Icon className="h-4 w-4" />
    </div>
  );
}
