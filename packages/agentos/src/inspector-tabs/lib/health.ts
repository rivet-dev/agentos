// Feature detection for the observe-only runtime-health actions. The queries
// themselves (healthQueryOptions / liveSessionsQueryOptions / cancelPrompt)
// live in lib/source.ts now that getRuntimeHealth / listSessions /
// cancelPrompt are real contract actions; this module keeps only the
// contract-layer detection callers use to hide health UI when a vendored tab
// bundle runs against an OLDER runtime that lacks the actions (those reject
// with a contract-layer InspectorActionError; the status strip hides itself
// and the composer disables its Stop button).
import { isInspectorActionError } from "./actor-client";

export function isMissingHealthAction(error: unknown): boolean {
	return isInspectorActionError(error) && error.layer === "contract";
}
