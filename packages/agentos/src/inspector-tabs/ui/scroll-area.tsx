// Vendored verbatim (trimmed) from the rivet design system. Self-contained:
// only @radix-ui/react-scroll-area + the local cn util.
import * as ScrollAreaPrimitive from "@radix-ui/react-scroll-area";
import * as React from "react";
import { cn } from "../lib/cn";

export const ScrollArea = React.forwardRef<
	React.ElementRef<typeof ScrollAreaPrimitive.Root>,
	React.ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.Root>
>(({ className, children, ...props }, ref) => (
	<ScrollAreaPrimitive.Root
		ref={ref}
		className={cn("relative overflow-hidden", className)}
		// "auto": the scrollbar is present whenever content overflows. The
		// default hover type mounts/unmounts it on scroll and layout events,
		// which reads as flashing next to polling content (sessions/health).
		type="auto"
		{...props}
	>
		<ScrollAreaPrimitive.Viewport className="h-full w-full rounded-[inherit] [&>div]:!block">
			{children}
		</ScrollAreaPrimitive.Viewport>
		<ScrollAreaPrimitive.ScrollAreaScrollbar
			orientation="vertical"
			className="flex h-full w-2.5 touch-none select-none border-l border-l-transparent p-[1px] transition-colors"
		>
			<ScrollAreaPrimitive.ScrollAreaThumb className="relative flex-1 rounded-full bg-border" />
		</ScrollAreaPrimitive.ScrollAreaScrollbar>
		<ScrollAreaPrimitive.Corner />
	</ScrollAreaPrimitive.Root>
));
ScrollArea.displayName = "ScrollArea";
