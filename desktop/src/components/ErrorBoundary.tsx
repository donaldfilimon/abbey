import { Component, type ReactNode } from "react";
export class ErrorBoundary extends Component<
  { children: ReactNode },
  { failed: boolean; generation: number; message: string }
> {
  override state = { failed: false, generation: 0, message: "" };
  /** Keeps the thrown value's message only: a stack never reaches the page. */
  static getDerivedStateFromError(thrown: unknown) {
    const message =
      thrown instanceof Error ? thrown.message : typeof thrown === "string" ? thrown : "";
    return { failed: true, message: message.slice(0, 500) };
  }
  override render() {
    return this.state.failed ? (
      <div className="recovery card">
        <h1>This view needs a fresh start</h1>
        <p className="muted">
          Abbey couldn’t display this window. Try reopening the local interface.
        </p>
        {this.state.message && (
          <p className="muted" role="alert">
            {this.state.message}
          </p>
        )}
        <button
          onClick={() =>
            this.setState((state) => ({
              failed: false,
              message: "",
              generation: state.generation + 1,
            }))
          }
        >
          Recover interface
        </button>
      </div>
    ) : (
      <div className="boundary-host" key={this.state.generation}>
        {this.props.children}
      </div>
    );
  }
}
