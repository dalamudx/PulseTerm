import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app";
import { initialize } from "./features/probe/controller";
import "./styles.css";

const container = document.getElementById("root");
if (!container) throw new Error("Missing root element");
createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
void initialize();
