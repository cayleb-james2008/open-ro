import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";

type Props = {
  panelName: string;
  children: ReactNode;
  onReset?: () => void;
};

type State = {
  hasError: boolean;
  message: string;
};

/** Isolates workbench panel crashes so a corrupt scene, script, or GUI doc recovers cleanly in place. */
export class PanelErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false, message: "" };

  static getDerivedStateFromError(error: Error): State {
    return {
      hasError: true,
      message: error?.message || "Unexpected rendering error.",
    };
  }

  componentDidCatch(_error: Error, _info: ErrorInfo) {
    /* Error state is surfaced via the inline recovery banner. */
  }

  private handleReset = () => {
    this.setState({ hasError: false, message: "" });
    this.props.onReset?.();
  };

  render() {
    if (this.state.hasError) {
      return (
        <div
          role="alert"
          data-error-boundary={this.props.panelName}
          className="flex min-h-[160px] flex-1 flex-col items-center justify-center gap-2.5 rounded-card border border-hairline bg-board/60 p-4 text-center"
        >
          <AlertTriangle className="size-5 text-pill-pro-fg" strokeWidth={1.5} aria-hidden />
          <div className="flex flex-col gap-1">
            <h3 className="text-sm font-medium text-ink">{this.props.panelName} hit a display error</h3>
            <p className="max-w-md break-words text-xs text-ink-2">
              {this.state.message || "The loaded project data could not be rendered cleanly."}
            </p>
          </div>
          <Button variant="outline" size="xs" onClick={this.handleReset} className="bg-surface max-md:min-h-11">
            <RotateCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Reset panel</span>
          </Button>
        </div>
      );
    }
    return this.props.children;
  }
}
