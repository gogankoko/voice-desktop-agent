import { Component, type ReactNode } from "react";
import { logUiError } from "../ipc/diagnostics";

interface State {
  failed: boolean;
}

/** Last-resort screen if the interface itself crashes: plain words and one clear way out. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error) {
    logUiError(`render crash: ${error.message}`);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <main className="main panel" role="alert">
        <h1>Something went wrong</h1>
        <p>Sorry, part of the assistant stopped working. Nothing on your computer was changed by this.</p>
        <button type="button" className="stop" onClick={() => window.location.reload()}>
          Restart the assistant
        </button>
      </main>
    );
  }
}
