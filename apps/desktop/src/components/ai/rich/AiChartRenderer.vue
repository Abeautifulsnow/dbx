<script setup lang="ts">
import { use } from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { LineChart, BarChart, PieChart } from "echarts/charts";
import { GridComponent, TooltipComponent, LegendComponent, TitleComponent } from "echarts/components";
import VChart from "vue-echarts";
import { useTheme } from "@/composables/useTheme";
import type { AiChartSpec } from "@/lib/ai/richContent/aiChartSpec";
import { buildAiChartOption } from "@/lib/ai/richContent/aiChartOption";

// Module-level `use([...])` is idempotent, so re-importing this component
// alongside QueryChart.vue is safe.
use([CanvasRenderer, LineChart, BarChart, PieChart, GridComponent, TooltipComponent, LegendComponent, TitleComponent]);

const props = defineProps<{
  spec: AiChartSpec;
}>();

const { isDark } = useTheme();
</script>

<template>
  <!-- Explicit non-zero height (>=320px) keeps an embedded chart from collapsing
       to zero height inside the message stream, and `autoresize` tracks panel
       resizes. -->
  <div class="my-2 overflow-hidden rounded-md border border-zinc-200 bg-zinc-50 dark:border-zinc-700/50 dark:bg-zinc-900">
    <VChart :option="buildAiChartOption(props.spec, { isDark })" autoresize class="h-80 w-full p-2" />
  </div>
</template>
