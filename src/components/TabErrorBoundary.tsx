import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = {
  tabTitle: string;
  children?: ReactNode;
};

type State = { error?: string };

export function tabErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Keeps a render failure in one tab from unmounting the whole window. React
 * unmounts the entire tree when an error escapes a render, so every tab layer
 * that can host a preview must sit inside its own boundary.
 */
export class TabErrorBoundary extends Component<Props, State> {
  state: State = {};

  static getDerivedStateFromError(error: unknown): State {
    return { error: tabErrorMessage(error) };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error(
      `Tab "${this.props.tabTitle}" failed to render:`,
      error,
      info.componentStack,
    );
  }

  private retry = (): void => {
    this.setState({ error: undefined });
  };

  render(): ReactNode {
    const { error } = this.state;
    if (error == null) return this.props.children;

    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 bg-background px-6 text-center">
        <div className="max-w-md text-sm text-foreground">
          <p className="font-medium">This tab could not be displayed</p>
          <p className="mt-1 break-words text-xs text-muted-foreground">
            {this.props.tabTitle}: {error}
          </p>
        </div>
        <button
          type="button"
          onClick={this.retry}
          className="rounded-md border border-border bg-accent px-3 py-1 text-xs text-accent-foreground hover:bg-accent/80"
        >
          Try again
        </button>
      </div>
    );
  }
}
