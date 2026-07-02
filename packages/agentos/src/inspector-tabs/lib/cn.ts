// Vendored from the rivet design system (@/components/lib/utils.ts → `cn`).
// Self-contained: only clsx + tailwind-merge.
import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}
