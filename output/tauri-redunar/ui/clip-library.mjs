// Basic card metadata and its thumbnail should not wait for a full packet scan.
export async function loadClipPreview({item, call, thumbnailDataUri, updateMetadata, updateThumbnail, selected}) {
  try {
    if (!item.metadata) {
      item.metadata = await call('clip_metadata', {fileName:item.file_name});
      if (Number.isFinite(item.metadata.duration_seconds) && item.metadata.duration_seconds>0) item.duration=item.metadata.duration_seconds;
      updateMetadata(item);
    }
  } catch { /* Leave retry possible on a later request. */ }
  if(item.thumbnail==null) {
    try { item.thumbnail=thumbnailDataUri(await call('clip_thumbnail',{fileName:item.file_name}))||''; }
    catch { item.thumbnail=''; }
  }
  updateThumbnail(item);
  if (selected(item) && !item.metadataDetailed) {
    try {
      item.metadata=await call('clip_metadata',{fileName:item.file_name,includeFps:true});
      item.metadataDetailed=true;
      if(selected(item)) updateMetadata(item);
    } catch { /* Basic metadata remains useful when a packet scan fails. */ }
  }
}
