import React, { Component, ErrorInfo, ReactNode } from "react";
import i18n from "../i18n";

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
    window.location.reload();
  };

  handleReset = () => {
    this.setState({ hasError: false, error: null });
  };

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) {
        return this.props.fallback;
      }

      return (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            height: "100%",
            padding: "24px",
            textAlign: "center",
            color: "var(--text-secondary, #666)",
          }}
        >
          <div
            style={{
              fontSize: "48px",
              marginBottom: "16px",
            }}
          >
            :(
          </div>
          <h2
            style={{
              margin: "0 0 8px 0",
              fontSize: "18px",
              fontWeight: 600,
              color: "var(--text-primary, #333)",
            }}
          >
            {i18n.t('layout:somethingWentWrong')}
          </h2>
          <p
            style={{
              margin: "0 0 24px 0",
              fontSize: "14px",
              maxWidth: "400px",
            }}
          >
            {this.state.error?.message || i18n.t('layout:unexpectedError')}
          </p>
          <div style={{ display: "flex", gap: "12px" }}>
            <button
              onClick={this.handleReset}
              style={{
                padding: "8px 16px",
                fontSize: "14px",
                border: "1px solid var(--border-color, #ddd)",
                borderRadius: "6px",
                background: "var(--bg-secondary, #f5f5f5)",
                color: "var(--text-primary, #333)",
                cursor: "pointer",
              }}
            >
              {i18n.t('layout:tryAgain')}
            </button>
            <button
              onClick={this.handleRefresh}
              style={{
                padding: "8px 16px",
                fontSize: "14px",
                border: "none",
                borderRadius: "6px",
                background: "var(--accent-color, #007AFF)",
                color: "#fff",
                cursor: "pointer",
              }}
            >
              {i18n.t('layout:refreshPage')}
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
