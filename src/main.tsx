import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "@/App";
import "@/styles.css";

const root = document.getElementById("root");

if (!root) {
  // If this fires, index.html and main.tsx have drifted apart.
  throw new Error("Root element #root is missing from index.html");
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
