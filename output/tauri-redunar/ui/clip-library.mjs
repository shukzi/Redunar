// Basic card metadata and its thumbnail should not wait for a full packet scan.
export async function loadClipPreview(options) {
  // A disk-cache hit should paint even while metadata inspection is still
  // pending. Keep optional packet counting after both basic card requests.
  // The bounded loader supplies the independently scheduled thumbnail phase.
  const thumbnail = options.thumbnail ?? loadThumbnail(options);
  await loadMetadata(options,thumbnail);
}

async function loadThumbnail({item, call, thumbnailDataUri, updateThumbnail}) {
  if(item.thumbnail==null) {
    try { item.thumbnail=thumbnailDataUri(await call('clip_thumbnail',{fileName:item.file_name}))||''; }
    catch { item.thumbnail=''; }
  }
  updateThumbnail(item);
}

async function loadMetadata({item, call, updateMetadata, selected},thumbnail=Promise.resolve()) {
  try {
    if (!item.metadata) {
      item.metadata = await call('clip_metadata', {fileName:item.file_name});
      if (Number.isFinite(item.metadata.duration_seconds) && item.metadata.duration_seconds>0) item.duration=item.metadata.duration_seconds;
      updateMetadata(item);
    }
  } catch { /* Leave retry possible on a later request. */ }
  await thumbnail;
  if (item.thumbnail!=null && selected(item) && !item.metadataDetailed) {
    try {
      item.metadata=await call('clip_metadata',{fileName:item.file_name,includeFps:true});
      item.metadataDetailed=true;
      if(selected(item)) updateMetadata(item);
    } catch { /* Basic metadata remains useful when a packet scan fails. */ }
  }
}

// Separate bounded lanes: slow metadata (including packet counting) must not
// occupy the slots needed to read the next cached images. Pending work always
// follows the latest visible cards; already-started native reads may finish.
function previewLane(needed,load,current) {
  let pending=[];
  const running=new Set();
  function pump() {
    while(running.size<2) {
      const index=pending.findIndex(item=>!running.has(item));
      if(index<0)return;
      const [item]=pending.splice(index,1);
      if(!current(item)||!needed(item))continue;
      running.add(item);
      Promise.resolve().then(()=>load(item)).catch(()=>{}).finally(()=>{running.delete(item);pump();});
    }
  }
  return {
    request(items){pending=[...new Set(items)];pump();},
    enqueue(item){if(!pending.includes(item))pending.push(item);pump();},
  };
}

export function clipPreviewLoader(options) {
  let visible=new Set();
  const current=item=>visible.has(item)&&options.current(item);
  const metadata=previewLane(
    item=>!item.metadata||(options.selected(item)&&!item.metadataDetailed),
    item=>loadClipPreview({...options,item,thumbnail:Promise.resolve()}),current,
  );
  const thumbnails=previewLane(item=>item.thumbnail==null,async item=>{
    await loadThumbnail({...options,item});
    // Basic inspection may have finished before the image. Only the selected
    // clip needs another pass to collect its optional detailed measurements.
    if(options.selected(item))metadata.enqueue(item);
  },current);
  return {
    request(items){visible=new Set(items);thumbnails.request(items);metadata.request(items);},
  };
}
