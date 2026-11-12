import assert from 'node:assert/strict';
import {effectiveMetrics,metricNames,metricWidth,overlayMetrics,telemetryWidth} from '../ui/overlay-metrics.mjs';
assert.equal(metricNames.length,17);
assert.equal(new Set(metricNames).size,17);
assert.deepEqual(new Set(effectiveMetrics('Detailed')),new Set([...effectiveMetrics('Compact'),'GPU power','CPU power']));
assert.equal(effectiveMetrics('Detailed').length,8);
assert.deepEqual(overlayMetrics.filter(([name])=>effectiveMetrics('Detailed').includes(name)).map(([name])=>name),['FPS','Frame time','GPU','GPU temperature','GPU power','CPU','CPU temperature','CPU power']);
assert.equal(effectiveMetrics('Compact').length,6);
assert.deepEqual(effectiveMetrics('FPS only'),['FPS']);
const selected=['CPU clock','CPU power','Graphics API'];
assert.deepEqual(effectiveMetrics('Custom',selected),selected);
assert.notEqual(effectiveMetrics('Custom',selected),selected);
assert.equal(telemetryWidth(['FPS']),300);
assert.equal(telemetryWidth(['GPU model']),400);
for(const [name,,value] of overlayMetrics){
 assert.ok(value.length*9+16<=metricWidth(name),`${name} preview value fits native cell`);
}
