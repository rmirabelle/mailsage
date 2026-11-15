import { invoke } from "@tauri-apps/api/core";
import { Component, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./index.css";

/**
 * Block the WebView's default context menu everywhere except text fields and
 * selectable text (error messages), which keep it for cut, copy, and paste.
 */
document.addEventListener("contextmenu", (event) => {
  const target = event.target as HTMLElement | null;
  if (target?.closest("input[type='text'], textarea, [contenteditable='true'], .selectable")) return;
  event.preventDefault();
});

/** Sends an error to the app's error output (the dev log); outside the app it does nothing. */
const logError = (message: string) => {
  invoke("log_frontend_error", { message }).catch(() => { /* Not running in the app. */ });
};
window.addEventListener("error", (event) => logError(`${event.message}\n${event.error?.stack ?? ""}`));
window.addEventListener("unhandledrejection", (event) => logError(`Unhandled rejection: ${event.reason?.stack ?? String(event.reason)}`));

/**
 * Dev only: the window reports each hot update it applied and each failed
 * one to the dev log (as "[client] ..."), so an edit can be confirmed as live.
 */
if (import.meta.hot) {
  const hot = import.meta.hot;
  const report = (message: string) => hot.send("mailsage:log", { message });
  hot.on("vite:afterUpdate", (payload) => report(`hot update applied: ${payload.updates.map((update) => update.path).join(", ")}`));
  hot.on("vite:error", (payload) => report(`hot update error: ${payload.err.message}`));
  report("window loaded");
}

/** A crash shows its error and a Reload button instead of a blank window, and goes to the dev log. */
class CrashScreen extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: { componentStack?: string | null }) {
    logError(`${error.stack ?? error.message}\nComponent stack:${info.componentStack ?? ""}`);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="crash-screen" role="alert">
        <h1>Mail Sage stopped with an error</h1>
        <pre className="selectable">{this.state.error.stack ?? this.state.error.message}</pre>
        <button className="button primary" onClick={() => window.location.reload()}>Reload</button>
      </div>
    );
  }
}

createRoot(document.getElementById("root")!).render(<CrashScreen><App /></CrashScreen>);
