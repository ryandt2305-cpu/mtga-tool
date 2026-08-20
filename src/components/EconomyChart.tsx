// Generic Chart.js line chart wrapper for SolidJS.
// Handles canvas lifecycle (mount/update/destroy) and applies QPM dark theme.

import { type Component, onMount, onCleanup, createEffect } from "solid-js";
import {
  Chart,
  LineController,
  LineElement,
  PointElement,
  BarController,
  BarElement,
  LinearScale,
  CategoryScale,
  Tooltip,
  Legend,
  Filler,
} from "chart.js";

Chart.register(
  LineController,
  LineElement,
  PointElement,
  BarController,
  BarElement,
  LinearScale,
  CategoryScale,
  Tooltip,
  Legend,
  Filler
);

export interface ChartDataset {
  label: string;
  data: number[];
  borderColor: string;
  backgroundColor?: string;
  yAxisID?: string;
  fill?: boolean;
  tension?: number;
  pointRadius?: number;
  borderWidth?: number;
}

export interface YAxisConfig {
  /** ID for the right-side Y axis */
  rightAxisId: string;
  /** Label for left axis (optional) */
  leftLabel?: string;
  /** Label for right axis (optional) */
  rightLabel?: string;
}

interface Props {
  datasets: ChartDataset[];
  labels: string[];
  yAxisConfig?: YAxisConfig;
  height?: number;
  type?: "line" | "bar";
}

const GRID_COLOR = "rgba(255, 255, 255, 0.06)";
const TICK_COLOR = "rgba(255, 255, 255, 0.4)";

/** External Chart.js tooltip — renders an HTML element portaled to <body> so
 *  it can extend past the chart canvas edge without being clipped, and gets
 *  clamped inside the viewport. */
function externalTooltip(context: { chart: Chart; tooltip: any }): void {
  const { chart, tooltip } = context;
  const id = "chartjs-external-tooltip";
  let el = document.getElementById(id);
  if (!el) {
    el = document.createElement("div");
    el.id = id;
    el.className = "chart-tooltip";
    el.style.position = "fixed";
    el.style.zIndex = "3000";
    el.style.pointerEvents = "none";
    el.style.opacity = "0";
    el.style.transition = "opacity 0.1s ease";
    document.body.appendChild(el);
  }
  if (tooltip.opacity === 0) { el.style.opacity = "0"; return; }

  const title = (tooltip.title ?? []).join(" ");
  const bodyLines: string[] = tooltip.body.flatMap((b: any) => b.lines);
  const labelColors: { backgroundColor: string; borderColor: string }[] = tooltip.labelColors ?? [];
  const rows = bodyLines.map((line, i) => {
    const c = labelColors[i];
    const swatch = c
      ? `<span class="chart-tooltip-swatch" style="background:${c.backgroundColor};border-color:${c.borderColor}"></span>`
      : "";
    const safe = line.replace(/[&<>]/g, (m: string) => (m === "&" ? "&amp;" : m === "<" ? "&lt;" : "&gt;"));
    return `<div class="chart-tooltip-row">${swatch}<span>${safe}</span></div>`;
  }).join("");
  el.innerHTML = `${title ? `<div class="chart-tooltip-title">${title.replace(/[&<>]/g, (m: string) => (m === "&" ? "&amp;" : m === "<" ? "&lt;" : "&gt;"))}</div>` : ""}${rows}`;

  const canvasRect = chart.canvas.getBoundingClientRect();
  const pad = 8;
  const gap = 12;
  const preferred = canvasRect.left + tooltip.caretX + gap;
  const w = el.offsetWidth;
  const h = el.offsetHeight;
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  let x = preferred;
  if (x + w > vw - pad) x = canvasRect.left + tooltip.caretX - gap - w;
  if (x < pad) x = pad;
  if (x + w > vw - pad) x = vw - pad - w;
  let y = canvasRect.top + tooltip.caretY - h / 2;
  if (y + h > vh - pad) y = vh - pad - h;
  if (y < pad) y = pad;

  el.style.left = `${x}px`;
  el.style.top = `${y}px`;
  el.style.opacity = "1";
}

const EconomyChart: Component<Props> = (props) => {
  let canvasRef!: HTMLCanvasElement;
  let chart: Chart | undefined;

  function buildConfig() {
    const scales: Record<string, any> = {
      x: {
        grid: { color: GRID_COLOR },
        ticks: { color: TICK_COLOR, maxRotation: 0, autoSkipPadding: 12, font: { size: 10 } },
        border: { display: false },
      },
      y: {
        position: "left" as const,
        grid: { color: GRID_COLOR },
        ticks: { color: TICK_COLOR, font: { size: 10 } },
        border: { display: false },
        beginAtZero: true,
      },
    };

    if (props.yAxisConfig) {
      scales.y.display = true;
      scales[props.yAxisConfig.rightAxisId] = {
        position: "right" as const,
        grid: { drawOnChartArea: false },
        ticks: { color: TICK_COLOR, font: { size: 10 } },
        border: { display: false },
        beginAtZero: true,
      };
    }

    const isBar = props.type === "bar";
    return {
      type: (isBar ? "bar" : "line") as "bar" | "line",
      data: {
        labels: props.labels,
        datasets: props.datasets.map((ds) => ({
          label: ds.label,
          data: ds.data,
          borderColor: ds.borderColor,
          backgroundColor: ds.backgroundColor ?? (isBar ? ds.borderColor : "transparent"),
          yAxisID: ds.yAxisID ?? "y",
          fill: ds.fill ?? false,
          tension: ds.tension ?? 0.3,
          pointRadius: ds.pointRadius ?? (props.labels.length > 50 ? 0 : 2),
          borderWidth: ds.borderWidth ?? (isBar ? 0 : 2),
          pointHoverRadius: 4,
        })),
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: {
          mode: "index" as const,
          intersect: false,
        },
        plugins: {
          legend: {
            labels: {
              color: TICK_COLOR,
              boxWidth: 12,
              font: { size: 11 },
            },
          },
          tooltip: {
            enabled: false,
            external: externalTooltip,
          },
        },
        scales,
      },
    };
  }

  onMount(() => {
    chart = new Chart(canvasRef, buildConfig());
  });

  // Update chart data reactively when props change
  createEffect(() => {
    if (!chart) return;
    const labels = props.labels;
    const datasets = props.datasets;

    const isBar = props.type === "bar";
    chart.data.labels = labels;
    chart.data.datasets = datasets.map((ds) => ({
      label: ds.label,
      data: ds.data,
      borderColor: ds.borderColor,
      backgroundColor: ds.backgroundColor ?? (isBar ? ds.borderColor : "transparent"),
      yAxisID: ds.yAxisID ?? "y",
      fill: ds.fill ?? false,
      tension: ds.tension ?? 0.3,
      pointRadius: ds.pointRadius ?? (labels.length > 50 ? 0 : 2),
      borderWidth: ds.borderWidth ?? (isBar ? 0 : 2),
      pointHoverRadius: 4,
    }));
    chart.update("none");
  });

  onCleanup(() => {
    chart?.destroy();
    chart = undefined;
  });

  return (
    <div class="history-chart-container" style={{ height: `${props.height ?? 280}px` }}>
      <canvas ref={canvasRef!} />
    </div>
  );
};

export default EconomyChart;
