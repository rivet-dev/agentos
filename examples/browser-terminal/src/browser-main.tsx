import { createRoot } from "react-dom/client";
import { BrowserApp } from "./BrowserApp";
import "./styles.css";

const el = document.getElementById("root");
if (!el) throw new Error("missing #root");
createRoot(el).render(<BrowserApp />);
