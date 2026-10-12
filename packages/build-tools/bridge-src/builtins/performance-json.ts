// Serialize the clock the VM actually provides. Native process/nodeTiming and
// event-loop utilization counters are not implemented and are not fabricated.
export function createPerformanceToJSON(performance) {
	return function toJSON() {
		if (this !== performance) {
			const error = new TypeError("Value of this must be of type Performance");
			error.code = "ERR_INVALID_THIS";
			throw error;
		}
		return { timeOrigin: performance.timeOrigin };
	};
}
