import type { LucideIcon } from "lucide-react";
import { Database, Globe, HardDrive, Monitor, Wrench } from "lucide-react";

// Registry entries reference icons by name (a serializable string) rather than
// by component. Passing a Lucide component through the Astro `client:load`
// island prop boundary mangles it (forwardRef objects don't survive devalue
// serialization), which throws on hydration and blanks the page. Resolve the
// name to a component in code on each side of the boundary instead.
export type RegistryIconName =
	| "HardDrive"
	| "Database"
	| "Monitor"
	| "Globe"
	| "Wrench";

export const REGISTRY_ICONS: Record<RegistryIconName, LucideIcon> = {
	HardDrive,
	Database,
	Monitor,
	Globe,
	Wrench,
};
