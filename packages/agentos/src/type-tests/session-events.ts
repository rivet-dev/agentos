import type { SessionStreamEntry } from "@rivet-dev/agentos-core";
import type { AgentOsEvents } from "../index.js";

type Equal<Left, Right> =
	(<Value>() => Value extends Left ? 1 : 2) extends <
		Value,
	>() => Value extends Right ? 1 : 2
		? true
		: false;
type Expect<Condition extends true> = Condition;

export type AgentOsEventsParity = Expect<
	Equal<AgentOsEvents["sessionEvent"], SessionStreamEntry>
>;
