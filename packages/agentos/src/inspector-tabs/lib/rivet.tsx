import {
	createContext,
	type ReactNode,
	useContext,
	useEffect,
	useMemo,
	useRef,
} from "react";
import {
	createAgentOsClient,
	type AgentOsActorConnection,
} from "../../generated/contract";
import { setRivetClient } from "./actor-client";

interface RivetValue {
	conn: AgentOsActorConnection | null;
}

const RivetContext = createContext<RivetValue | null>(null);

export function RivetProvider({
	actorId,
	authToken,
	children,
}: {
	actorId: string;
	authToken: string;
	children: ReactNode;
}) {
	const value = useMemo<RivetValue>(() => {
		const client = createAgentOsClient({
			endpoint: window.location.origin,
			token: authToken,
			encoding: "bare",
			disableMetadataLookup: true,
		});
		setRivetClient(client, actorId);
		try {
			return { conn: client.getForId("agentOS", actorId).connect() };
		} catch (error) {
			console.error("agentOS inspector: failed to connect to actor events", error);
			return { conn: null };
		}
	}, [actorId, authToken]);

	useEffect(
		() => () => {
			try {
				value.conn?.dispose();
			} catch (error) {
				console.warn("agentOS inspector: failed to dispose event connection", error);
			}
		},
		[value],
	);

	return <RivetContext.Provider value={value}>{children}</RivetContext.Provider>;
}

function useAgentOsEvent(
	conn: AgentOsActorConnection | null,
	name: string,
	handler: (payload: unknown) => void,
): void {
	const handlerRef = useRef(handler);
	handlerRef.current = handler;
	useEffect(() => {
		if (!conn) return;
		const events = conn as unknown as {
			on(name: string, callback: (payload: unknown) => void): unknown;
		};
		const eventName = EVENT_NAMES[name] ?? name;
		const unsubscribe = events.on(eventName, (payload) =>
			handlerRef.current(mapEventPayload(name, payload)),
		);
		return () => {
			if (typeof unsubscribe === "function") unsubscribe();
		};
	}, [conn, name]);
}

const EVENT_NAMES: Record<string, string> = {
	vmBooted: "vm.booted",
	vmShutdown: "vm.shutdown",
	processOutput: "process.output",
	processExit: "process.exit",
	shellData: "terminal.output",
	shellExit: "terminal.exit",
};

function mapEventPayload(name: string, payload: unknown): unknown {
	if (typeof payload !== "object" || payload === null) return payload;
	if (name === "processOutput") {
		const event = payload as {
			process: { pid: number };
			sequence: number | bigint;
			stream: string;
			data: unknown;
		};
		return {
			pid: event.process.pid,
			seq: Number(event.sequence),
			stream: event.stream,
			data: event.data,
		};
	}
	if (name === "processExit") {
		const event = payload as {
			process: { pid: number };
			status: { exitCode: number };
		};
		return { pid: event.process.pid, exitCode: event.status.exitCode };
	}
	if (name === "shellData") {
		const event = payload as {
			terminal: { shellId: string };
			sequence: number | bigint;
			data: unknown;
		};
		return {
			shellId: event.terminal.shellId,
			seq: event.sequence,
			data: event.data,
		};
	}
	if (name === "shellExit") {
		const event = payload as {
			terminal: { shellId: string };
			status: { exitCode: number };
		};
		return {
			shellId: event.terminal.shellId,
			exitCode: event.status.exitCode,
		};
	}
	return payload;
}

export function useAgentOsActor() {
	const context = useContext(RivetContext);
	if (!context) {
		throw new Error("useAgentOsActor must be used within RivetProvider");
	}
	return {
		useEvent: (name: string, handler: (payload: unknown) => void) =>
			useAgentOsEvent(context.conn, name, handler),
	};
}
