import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { RenderErrorBoundary } from "./components/render-error-boundary";
import { reportRendererError } from "./lib/desktopHost";
import "./styles.css";

function errorPayload(kind: string, value: unknown) {
  const error = value instanceof Error ? value : null;
  return {
    kind,
    message: error?.message ?? String(value),
    stack: error?.stack,
    href: window.location.href,
    userAgent: navigator.userAgent,
  };
}

window.addEventListener("error", (event) => {
  void reportRendererError({
    ...errorPayload("window-error", event.error ?? event.message),
    source: event.filename,
    line: event.lineno,
    column: event.colno,
  });
});

window.addEventListener("unhandledrejection", (event) => {
  void reportRendererError(errorPayload("unhandled-rejection", event.reason));
});

const root = document.getElementById("root");
if (!root) {
  void reportRendererError({
    kind: "bootstrap",
    message: "Missing #root element",
    href: window.location.href,
    userAgent: navigator.userAgent,
  });
  document.body.textContent = "Cerul failed to start: missing root element.";
} else {
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <RenderErrorBoundary>
        <App />
      </RenderErrorBoundary>
    </React.StrictMode>,
  );
}
