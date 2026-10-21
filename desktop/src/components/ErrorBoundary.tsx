import { Component, type ReactNode } from "react";
export class ErrorBoundary extends Component<
  { children: ReactNode },
  { failed: boolean; generation: number }
> {
  override state = { failed: false, generation: 0 };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  override render() {
    return this.state.failed ? (
      <div className="recovery card">
        <h1>This view needs a fresh start</h1>
        <p className="muted">
          Abbey couldn’t display this window. Try reopening the local interface.
        </p>
        <button
          onClick={() =>
            this.setState((state) => ({
              failed: false,
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
