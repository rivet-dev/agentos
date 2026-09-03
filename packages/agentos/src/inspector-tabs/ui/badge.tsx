// Minimal Badge vendored from the rivet design system. The original imports a
// `./helpers` design-system util (CommonHelperProps) + premium-gradient
// variants we don't use; this trims to just the `secondary` / `outline`
// variants the inspector tabs need, avoiding any design-system drag.
import { cva, type VariantProps } from "class-variance-authority";
import React from "react";
import { cn } from "../lib/cn";

const badgeVariants = cva(
	"inline-flex items-center tracking-normal rounded-full border px-2.5 py-0.5 text-xs font-semibold transition-colors whitespace-nowrap max-w-full overflow-hidden truncate",
	{
		variants: {
			variant: {
				secondary: "border-transparent bg-secondary text-secondary-foreground",
				outline: "text-foreground",
			},
		},
		defaultVariants: { variant: "secondary" },
	},
);

export interface BadgeProps
	extends React.HTMLAttributes<HTMLDivElement>,
		VariantProps<typeof badgeVariants> {}

export function Badge({ className, variant, ...props }: BadgeProps) {
	return <div className={cn(badgeVariants({ variant }), className)} {...props} />;
}
