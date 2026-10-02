export function canRetryShortcuts(status, savedBindings) {
  return ['Inactive','Unavailable','Denied'].includes(status?.state)
    && savedBindings.some(binding=>typeof binding==='string' && binding.trim());
}
