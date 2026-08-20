import { type Component, For, Show, createSignal } from "solid-js";
import Header from "../components/Header";
import { diagnostics, schemaWarnings, serviceStatuses } from "../stores/appStore";
import { exportDiagnostics } from "../lib/tauri";

function stateToColor(state: string): string {
  switch (state) {
    case "ok":
    case "active":
      return "var(--color-status-ok)";
    case "scanning":
      return "var(--color-status-scanning)";
    case "error":
      return "var(--color-status-error)";
    default:
      return "var(--color-status-idle)";
  }
}

function levelToColor(level: string): string {
  switch (level) {
    case "error":
      return "var(--color-status-error)";
    case "warn":
      return "var(--color-status-warn)";
    default:
      return "var(--color-border)";
  }
}

const Settings: Component = () => {
  const [copyLabel, setCopyLabel] = createSignal("Copy Debug Info");

  async function copyDebugInfo() {
    try {
      const exported = await exportDiagnostics();
      const ts = new Date(exported.timestamp_epoch_s * 1000).toISOString();
      const warns = schemaWarnings();
      const diags = diagnostics();

      const lines = [
        `MTGA Hub v${exported.app_version}`,
        `Timestamp: ${ts}`,
        "",
        "## Services",
        ...exported.services.map(
          (s) =>
            `[${s.state}] ${s.name}: ${s.detail}${s.path ? ` (${s.path})` : ""}`,
        ),
        "",
        `## Schema Warnings (${warns.length})`,
        ...warns.map((w) => `${w.code}: ${w.message}`),
        "",
        `## Recent Diagnostics (${Math.min(diags.length, 20)})`,
        ...diags
          .slice(-20)
          .reverse()
          .map((d) => `[${d.level}] ${d.code}: ${d.message}`),
      ];

      await navigator.clipboard.writeText(lines.join("\n"));
      setCopyLabel("Copied!");
      setTimeout(() => setCopyLabel("Copy Debug Info"), 1500);
    } catch {
      setCopyLabel("Failed");
      setTimeout(() => setCopyLabel("Copy Debug Info"), 1500);
    }
  }

  return (
    <div class="view">
      <Header title="Settings" />
      <div class="view-content">
        {/* Services */}
        <section class="settings-section">
          <div class="settings-section-header">
            <h2 class="settings-section-title">Services</h2>
          </div>
          <div class="service-list">
            <For each={serviceStatuses()}>
              {(svc) => (
                <div class="service-row">
                  <span
                    class={`service-dot${svc.state === "scanning" ? " scanning" : ""}`}
                    style={{ background: stateToColor(svc.state) }}
                  />
                  <span class="service-name">{svc.name}</span>
                  <span class="service-detail">{svc.detail}</span>
                  <Show when={svc.path}>
                    <span class="service-path" title={svc.path!}>
                      {svc.path}
                    </span>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </section>

        {/* Diagnostics */}
        <section class="settings-section">
          <div class="settings-section-header">
            <h2 class="settings-section-title">Diagnostics</h2>
            <button class="copy-btn" onClick={copyDebugInfo}>
              {copyLabel()}
            </button>
          </div>
          <Show
            when={diagnostics().length > 0}
            fallback={
              <p class="diagnostics-empty">No diagnostic events yet.</p>
            }
          >
            <div class="diagnostics-scroll">
              <For each={diagnostics().slice().reverse().slice(0, 50)}>
                {(d) => (
                  <div
                    class="diagnostic-entry"
                    style={{
                      "border-left-color": levelToColor(d.level),
                    }}
                  >
                    <span class="diagnostic-code">{d.code}</span>
                    <span class="diagnostic-message">{d.message}</span>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </section>

        {/* Schema Drift */}
        <Show when={schemaWarnings().length > 0}>
          <section class="settings-section">
            <div class="settings-section-header">
              <h2 class="settings-section-title">Schema Drift</h2>
            </div>
            <div class="schema-warnings">
              <For each={schemaWarnings()}>
                {(w) => (
                  <div class="schema-warning">
                    <span class="schema-warning-code">{w.code}</span>
                    <span class="schema-warning-message">{w.message}</span>
                    <Show when={w.details.length > 0}>
                      <ul class="schema-warning-details">
                        <For each={w.details}>
                          {(detail) => <li>{detail}</li>}
                        </For>
                      </ul>
                    </Show>
                  </div>
                )}
              </For>
            </div>
          </section>
        </Show>
      </div>
    </div>
  );
};

export default Settings;
