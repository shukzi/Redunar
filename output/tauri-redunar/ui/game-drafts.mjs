const keys = ['overlay', 'captureMetrics'];
const value = (overrides, key) => Object.hasOwn(overrides || {}, key) ? overrides[key] : null;

// Catalog writes return every game. Merge saved data without treating an
// unrelated entry's pending profile as if the user had discarded it.
export function mergeGameDrafts(saved, current, baselines, committedId = null) {
  const previous = new Map(current.map(game => [game.id, game]));
  return saved.map(game => {
    const old = previous.get(game.id);
    const dirty = old && game.id !== committedId
      ? keys.filter(key => value(old.overrides, key) !== value(baselines.get(game.id), key)) : [];
    const merged = {...game, overrides: {...game.overrides}, savedRevision: game.revision};
    for (const key of dirty) {
      if (Object.hasOwn(old.overrides, key)) merged.overrides[key] = old.overrides[key];
      else delete merged.overrides[key];
    }
    // A concurrent profile writer must still trigger the native conflict check.
    if (dirty.length) merged.revision = old.revision;
    return merged;
  });
}
