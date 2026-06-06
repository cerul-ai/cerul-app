import React from "react";
import ReactDOM from "react-dom/client";
import { OverlayApp } from "./OverlayApp";
import { LangProvider } from "./lib/i18n";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <LangProvider>
      <OverlayApp />
    </LangProvider>
  </React.StrictMode>,
);
