import { Component, type ErrorInfo, type ReactNode } from 'react';

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/** Keeps a render failure on screen instead of replacing the studio with a blank document. */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error(error.message, info.componentStack);
  }

  render(): ReactNode {
    if (!this.state.error) {
      return this.props.children;
    }
    return (
      <main id="main" className="content-page" role="alert">
        <h1>The studio hit a renderer error</h1>
        <p className="test-result failed">{this.state.error.message}</p>
        <button type="button" className="primary-btn" onClick={() => this.setState({ error: null })}>Try again</button>
      </main>
    );
  }
}
