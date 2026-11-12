"use strict";

// Undici imports this while the bridge bundle is still bootstrapping, before
// `_asyncHooksModule` exists. Undici's classes extend AsyncResource at load time,
// so look up the real module only when an instance is made or used.
function asyncHooks() {
	return globalThis._asyncHooksModule;
}

class AsyncResource {
	constructor(type = "AgentOsAsyncResource") {
		this.type = type;
		this._resource = new (asyncHooks().AsyncResource)(type);
	}

	bind(fn, thisArg = undefined) {
		return this._resource.bind(fn, thisArg);
	}

	emitDestroy() {
		return this;
	}

	runInAsyncScope(fn, thisArg, ...args) {
		return this._resource.runInAsyncScope(fn, thisArg, ...args);
	}
}

module.exports = {
	get AsyncLocalStorage() {
		return asyncHooks().AsyncLocalStorage;
	},
	AsyncResource,
	createHook(...args) {
		return asyncHooks().createHook(...args);
	},
	executionAsyncId() {
		return asyncHooks().executionAsyncId();
	},
	triggerAsyncId() {
		return asyncHooks().triggerAsyncId();
	},
};
