import { createSignal, onMount, Show, type Component } from "solid-js";
import type { Update } from "@tauri-apps/plugin-updater";
import {
  checkForUpdate,
  downloadAndInstall,
  relaunchApp,
  type UpdateInfo,
} from "../lib/updater";

type Phase = "idle" | "available" | "downloading" | "ready" | "error" | "dismissed";

const UpdateBanner: Component = () => {
  const [phase, setPhase] = createSignal<Phase>("idle");
  const [info, setInfo] = createSignal<UpdateInfo | null>(null);
  const [update, setUpdate] = createSignal<Update | null>(null);
  const [progress, setProgress] = createSignal(0);
  const [errorMsg, setErrorMsg] = createSignal("");

  onMount(async () => {
    const result = await checkForUpdate();
    if (!result) return;
    setUpdate(result.update);
    setInfo(result.info);
    setPhase("available");
  });

  const install = async () => {
    const u = update();
    if (!u) return;
    setPhase("downloading");
    setProgress(0);
    try {
      await downloadAndInstall(u, ({ downloaded, total }) => {
        if (total && total > 0) {
          setProgress(Math.min(100, Math.round((downloaded / total) * 100)));
        }
      });
      setPhase("ready");
    } catch (err) {
      setErrorMsg(String(err));
      setPhase("error");
    }
  };

  const restart = async () => {
    try {
      await relaunchApp();
    } catch (err) {
      setErrorMsg(String(err));
      setPhase("error");
    }
  };

  return (
    <Show when={phase() !== "idle" && phase() !== "dismissed"}>
      <div class="update-banner" role="status" aria-live="polite">
        <Show when={phase() === "available"}>
          <span class="update-banner-icon" aria-hidden="true">⬆</span>
          <span class="update-banner-text">
            MTGA Hub <strong>{info()?.version}</strong> available
            <Show when={info()?.currentVersion}>
              <span class="update-banner-current"> · you're on {info()?.currentVersion}</span>
            </Show>
          </span>
          <button class="update-banner-btn primary" onClick={install}>
            Install
          </button>
          <button class="update-banner-btn" onClick={() => setPhase("dismissed")}>
            Later
          </button>
        </Show>

        <Show when={phase() === "downloading"}>
          <span class="update-banner-icon" aria-hidden="true">⬇</span>
          <span class="update-banner-text">
            Downloading {info()?.version}… {progress()}%
          </span>
          <div class="update-banner-progress">
            <div class="update-banner-progress-fill" style={{ width: `${progress()}%` }} />
          </div>
        </Show>

        <Show when={phase() === "ready"}>
          <span class="update-banner-icon" aria-hidden="true">✓</span>
          <span class="update-banner-text">
            Update installed. Restart to finish.
          </span>
          <button class="update-banner-btn primary" onClick={restart}>
            Restart now
          </button>
          <button class="update-banner-btn" onClick={() => setPhase("dismissed")}>
            Later
          </button>
        </Show>

        <Show when={phase() === "error"}>
          <span class="update-banner-icon error" aria-hidden="true">!</span>
          <span class="update-banner-text">
            Update failed: {errorMsg()}
          </span>
          <button class="update-banner-btn" onClick={() => setPhase("dismissed")}>
            Dismiss
          </button>
        </Show>
      </div>
    </Show>
  );
};

export default UpdateBanner;
