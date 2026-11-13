import React from "react";
import ReactDOM from "react-dom/client";
import "../../../src/styles.css";
import "../../../src/fleet.css";
import "../../../src/mobile.css";

// This must precede importing App/runtime: a phone never boots the local desktop backend.
window.__MINERDESK_MOBILE__ = true;
void import("../../../src/MobileConnection").then(({ default: MobileConnection }) => {
  document.documentElement.classList.add("mobile-client");
  ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><MobileConnection/></React.StrictMode>);
}).catch(() => {
  const root = document.getElementById("root");
  if (root) root.textContent = "MinerDesk Mobile could not open. Close and reopen the app.";
});
