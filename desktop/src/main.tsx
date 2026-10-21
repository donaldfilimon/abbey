import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles.css";
import "./presentation/material.css";
import { ErrorBoundary } from "./components/ErrorBoundary";

const host = document.getElementById("root");
if (!host) {
  throw new Error("missing #root mount point");
}

createRoot(host).render(
  <StrictMode>
    <ErrorBoundary><App /></ErrorBoundary>
  </StrictMode>,
);
