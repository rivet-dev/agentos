# Node internal bindings

This directory owns the small AgentOS adapters for Node's `internalBinding()`
contract. Vendored `vendor/lib/**/*.js` files are never edited. Binding behavior
is implemented here or below the seam in the V8 runtime, bridge, kernel, and
shared WASM leaf libraries.
