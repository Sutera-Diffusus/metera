import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles.css";
import { DashboardApp } from "./DashboardApp";
import { FloatingMeter } from "./FloatingMeter";
import { MeteraProvider } from "./state/MeteraContext";

function route() {
  const query = new URLSearchParams(location.search).get("window");
  if (query) return query;
  try { return getCurrentWindow().label; } catch { return innerWidth < 500 ? "floating-meter" : "dashboard"; }
}

const current = route();
document.documentElement.dataset.window = current;
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <MeteraProvider initialRange={current === "floating-meter" ? "7d" : "today"}>
      {current === "floating-meter" ? <FloatingMeter /> : <DashboardApp />}
    </MeteraProvider>
  </React.StrictMode>,
);
