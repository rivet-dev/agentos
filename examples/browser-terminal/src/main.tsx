import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles.css";

// Note: intentionally not wrapped in <StrictMode>. Its dev-only double-mount
// would tear down and recreate each xterm terminal, which is disruptive for a
// live PTY session.
const el = document.getElementById("root");
if (!el) throw new Error("missing #root");
createRoot(el).render(<App />);
