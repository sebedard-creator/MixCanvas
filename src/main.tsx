import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import "./app.css";

const rootElement = document.getElementById("root");

if (!rootElement) {
  throw new Error("Le point de montage React est introuvable.");
}

createRoot(rootElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
