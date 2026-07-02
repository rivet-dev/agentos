
import { Component, type ReactNode, Suspense } from "react";
import React from "react";

function TabFallback() {
	return (
		<div className="flex h-full flex-1 items-center justify-center p-8 text-sm text-muted-foreground">
			<span className="inline-flex items-center gap-2">
				<svg className="size-4 animate-spin text-muted-foreground" viewBox="0 0 24 24" fill="none" aria-hidden="true">
					<circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" />
					<path className="opacity-75" fill="currentColor" d="M12 2a10 10 0 0 1 10 10h-3a7 7 0 0 0-7-7z" />
				</svg>
				Loading…
			</span>
		</div>
	);
}

class ErrorBoundary extends Component<{ children: ReactNode }, { error?: Error }> {
	state: { error?: Error } = {};
	static getDerivedStateFromError(error: Error) {
		return { error };
	}
	render() {
		if (this.state.error) {
			return (
				<div className="flex h-full flex-1 items-center justify-center p-8 text-center text-sm text-destructive">
					{this.state.error.message || "Failed to load."}
				</div>
			);
		}
		return this.props.children;
	}
}

export function TabBoundary({ children }: { children: ReactNode }) {
	return (
		<ErrorBoundary>
			<Suspense fallback={<TabFallback />}>{children}</Suspense>
		</ErrorBoundary>
	);
}
