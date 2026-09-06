/**
 * UI adapter from a normalized {@link AiChartSpec} to an ECharts option.
 * Decoupled from the wire protocol: only `buildAiChartOption` knows how a spec
 * maps onto ECharts, so the protocol layer never needs to reason about
 * rendering and the renderer never sees the raw chart-json.
 */

import type { EChartsOption } from "echarts";
import type { AiChartSpec } from "@/lib/ai/richContent/aiChartSpec";

export interface AiChartTheme {
  isDark: boolean;
}

function axisLabelColor(theme: AiChartTheme): string {
  return theme.isDark ? "#aaa" : "#666";
}

function legendTextStyle(theme: AiChartTheme): { color: string } {
  return { color: theme.isDark ? "#ccc" : "#333" };
}

export function buildAiChartOption(spec: AiChartSpec, theme: AiChartTheme): EChartsOption {
  if (spec.type === "pie") {
    return {
      tooltip: { trigger: "item" },
      legend: { bottom: 0, textStyle: legendTextStyle(theme) },
      title: spec.title ? { text: spec.title, left: "center", textStyle: { color: theme.isDark ? "#ccc" : "#333", fontSize: 13 } } : undefined,
      series: [
        {
          type: "pie",
          radius: ["30%", "60%"],
          data: spec.data.map((entry) => ({ name: entry.name, value: entry.value })),
        },
      ],
    };
  }

  return {
    tooltip: { trigger: "axis" },
    legend: { bottom: 0, textStyle: legendTextStyle(theme) },
    title: spec.title ? { text: spec.title, left: "center", textStyle: { color: theme.isDark ? "#ccc" : "#333", fontSize: 13 } } : undefined,
    grid: { left: 60, right: 20, top: spec.title ? 40 : 20, bottom: 40 },
    xAxis: {
      type: "category",
      name: spec.xAxis.label ?? undefined,
      nameTextStyle: { color: axisLabelColor(theme) },
      data: spec.xAxis.values,
      axisLabel: { color: axisLabelColor(theme) },
    },
    yAxis: {
      type: "value",
      name: spec.yAxis?.label ?? undefined,
      nameTextStyle: { color: axisLabelColor(theme) },
      axisLabel: { color: axisLabelColor(theme) },
    },
    series: spec.series.map((series) => ({
      name: series.name,
      type: spec.type,
      data: series.data,
      smooth: spec.type === "line",
    })),
  };
}
