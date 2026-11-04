import React, { Component, ErrorInfo, ReactNode } from "react";
import i18n from "../i18n";
import { buttonVariants } from "./ui/button-variants";
import { reloadPage } from "../features/connect/reloadPage";

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("ErrorBoundary caught an error:", error, errorInfo);
  }

  handleRefresh = () => {
    reloadPage();
  };

  handleReset = () => {
    this.setState({ hasError: false, error: null });
  };

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) {
        return this.props.fallback;
      }

      // Theme tokens and the app's own keys, so the page reads in either theme.
      return (
        <div
          className="flex h-full flex-col items-center justify-center p-6 text-center text-[var(--color-text-secondary)]"
          role="alert"
        >
          <div className="mb-4 text-5xl" aria-hidden="true">
            :(
          </div>
          <h2 className="mb-2 text-lg font-semibold text-[var(--color-text-primary)]">
            {i18n.t("layout:somethingWentWrong")}
          </h2>
          <p className="mb-6 max-w-[400px] text-sm">
            {this.state.error?.message || i18n.t("layout:unexpectedError")}
          </p>
          <div className="flex gap-3">
            <button
              type="button"
              onClick={this.handleReset}
              className={buttonVariants({ variant: "outline" })}
              data-lcd-key
            >
              {i18n.t("layout:tryAgain")}
            </button>
            <button
              type="button"
              onClick={this.handleRefresh}
              className={buttonVariants()}
              data-lcd-primary
            >
              {i18n.t("layout:refreshPage")}
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
