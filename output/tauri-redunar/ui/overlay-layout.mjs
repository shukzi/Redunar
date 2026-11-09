export const FPS_ONLY = 'FPS only';
export const overlayEdgeSpacing = {defaultPixels:12,maxPixels:96,stepPixels:4};
export const overlayLayouts = ['Ribbon', 'Telemetry', FPS_ONLY];
export const panelPresets = ['Compact', 'Detailed', 'Custom'];

export function selectedOverlayLayout({layout, preset}) {
 return preset===FPS_ONLY ? FPS_ONLY : layout||'Telemetry';
}

export function panelPreset({preset}, fallback='Compact') {
 return panelPresets.includes(preset) ? preset : fallback;
}

export function selectOverlayLayout(draft, layout, previousPanelPreset) {
 const remembered=panelPreset(draft,previousPanelPreset);
 if(layout===FPS_ONLY){
  draft.preset=FPS_ONLY;
 }else{
  draft.layout=layout;
  draft.preset=remembered;
 }
 return remembered;
}
