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
            backgroundColor: "rgba(22, 22, 29, 0.95)",
            titleColor: "#e8e6f0",
            bodyColor: "#9995b0",
            borderColor: "rgba(255, 255, 255, 0.08)",
            borderWidth: 1,
            padding: 10,
            cornerRadius: 6,
            bodyFont: { size: 11 },
            titleFont: { size: 12 },
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
