// Some WebKit failures never emit a media error. Bound the wait for a decoded
// first frame, and detach the deadline when this selection is ready or gone.
export function watchReplayLoading(video, signal, onTimeout, timeoutMs = 15000) {
  let timer;
  const clear = () => {
    clearTimeout(timer);
    video.removeEventListener('loadeddata', clear);
    video.removeEventListener('error', clear);
    signal.removeEventListener('abort', clear);
  };
  if (signal.aborted) return;
  video.addEventListener('loadeddata', clear, {once: true});
  video.addEventListener('error', clear, {once: true});
  signal.addEventListener('abort', clear, {once: true});
  timer = setTimeout(() => { clear(); onTimeout(); }, timeoutMs);
}
