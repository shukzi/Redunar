// Order, labels and sample values for the settings preview. Samples are not telemetry.
export const overlayMetrics = [
 ['FPS','FPS','144 FPS'],
 ['Frame time','FRAME TIME','6.9 MS'],
 ['1% low','1% LOW','118 FPS'],
 ['0.1% low','0.1% LOW','96 FPS'],
 ['Graphics API','API','VULKAN'],
 ['GPU model','GPU MODEL','NVIDIA GEFORCE RTX 5070'],
 ['GPU','GPU LOAD','91%'],
 ['GPU temperature','GPU TEMP','68°C'],
 ['GPU power','GPU POWER','177.6 W'],
 ['GPU clock','GPU CLOCK','2835 MHz'],
 ['VRAM','VRAM','8319 MiB'],
 ['CPU model','CPU MODEL','AMD RYZEN 7 9800X3D'],
 ['CPU','CPU LOAD','38%'],
 ['CPU temperature','CPU TEMP','62°C'],
 ['CPU power','CPU POWER','65.6 W'],
 ['CPU clock','CPU CLOCK','5225 MHz'],
 ['RAM','RAM','15048 MiB'],
];
export const metricNames=overlayMetrics.map(([name])=>name);
const compact=['FPS','Frame time','GPU','CPU','GPU temperature','CPU temperature'];
export const presetMetrics=preset=>preset==='Compact'?[...compact]:preset==='Detailed'?[...compact,'GPU power','CPU power']:preset==='FPS only'?['FPS']:[];
export const effectiveMetrics=(preset,custom=[])=>preset==='Custom'?[...custom]:presetMetrics(preset||'Compact');
export const metricWidth=metric=>metric.endsWith('model')?304:compact.includes(metric)||metric==='1% low'||metric==='0.1% low'?89:128;
export const telemetryWidth=metrics=>metrics.some(metric=>metric.endsWith('model'))?400:300;
// Display labels stay separate from persisted metric identifiers.
export const metricLabel=metric=>metric==='CPU'?'CPU load':metric==='GPU'?'GPU load':metric;
export const metricDescriptions={
 'Graphics API':'Vulkan or OpenGL, not the original DirectX API under Proton.',
};
