import { createRoot } from "react-dom/client";
import "./index.css";
import { App } from "./App";
import { PanelErrorBoundary } from "./components/PanelErrorBoundary";

createRoot(document.getElementById("root")!).render(
  <PanelErrorBoundary panelName="OpenRO Studio" onReset={() => window.location.reload()}>
    <App />
  </PanelErrorBoundary>,
);
