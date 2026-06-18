import { AlertTriangle, Copy, RefreshCcw } from "lucide-react";
import React from "react";
import { reportRendererError } from "../lib/desktopHost";

type RenderErrorBoundaryState = {
  error: Error | null;
  componentStack: string | null;
  copied: boolean;
};

export class RenderErrorBoundary extends React.Component<
  { children: React.ReactNode },
  RenderErrorBoundaryState
> {
  state: RenderErrorBoundaryState = {
    error: null,
    componentStack: null,
    copied: false,
  };

  static getDerivedStateFromError(error: Error): Partial<RenderErrorBoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    this.setState({ componentStack: info.componentStack ?? null });
    void reportRendererError({
      kind: "react-render",
      message: error.message,
      stack: error.stack,
      componentStack: info.componentStack,
      href: window.location.href,
      userAgent: navigator.userAgent,
    });
  }

  private diagnostics() {
    const { error, componentStack } = this.state;
    return [
      "== Cerul renderer error ==",
      `time=${new Date().toISOString()}`,
      `url=${window.location.href}`,
      `userAgent=${navigator.userAgent}`,
      "",
      error?.stack ?? error?.message ?? "Unknown renderer error",
      componentStack ? `\n== React component stack ==\n${componentStack}` : "",
    ]
      .filter(Boolean)
      .join("\n");
  }

  private copyDiagnostics = async () => {
    try {
      await navigator.clipboard.writeText(this.diagnostics());
      this.setState({ copied: true });
      window.setTimeout(() => this.setState({ copied: false }), 1800);
    } catch {
      this.setState({ copied: false });
    }
  };

  render() {
    const { error, copied } = this.state;
    if (!error) {
      return this.props.children;
    }

    return (
      <main className="render-error-page" role="alert">
        <section className="render-error-card">
          <div className="render-error-icon">
            <AlertTriangle size={24} />
          </div>
          <div className="render-error-copy">
            <p className="mono-eyebrow">CERUL RENDERER</p>
            <h1>界面加载失败</h1>
            <p>
              Cerul 的窗口进程还在运行，但前端渲染遇到了错误。你可以先重新加载界面；
              如果再次出现，请复制诊断信息。
            </p>
          </div>
          <pre className="render-error-detail">{error.message}</pre>
          <div className="render-error-actions">
            <button type="button" className="btn btn-primary" onClick={() => window.location.reload()}>
              <RefreshCcw size={16} />
              <span>重新加载</span>
            </button>
            <button type="button" className="btn btn-secondary" onClick={this.copyDiagnostics}>
              <Copy size={16} />
              <span>{copied ? "已复制" : "复制诊断"}</span>
            </button>
          </div>
        </section>
      </main>
    );
  }
}
