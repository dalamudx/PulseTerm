import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app";
import { createProbeViewHost, initialize } from "./features/probe/controller";
import "./styles.css";

const container = document.getElementById("root");
if (!container) throw new Error("Missing root element");
const life = new AbortController();
const viewHost = createProbeViewHost(document, life.signal);
window.addEventListener("pagehide", () => life.abort(), { once: true });
createRoot(container).render(
  <StrictMode>
    <App viewHost={viewHost} />
  </StrictMode>,
);
void initialize();
