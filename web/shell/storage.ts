// Local storage can be unavailable (privacy modes); every preference then falls back to
// its default and is kept for the page's lifetime only.
export function storedValue(key: string, fallback: string) {
  try {
    return localStorage.getItem(key) || fallback;
  } catch (_error) {
    return fallback;
  }
}

export function storeValue(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch (_error) {
    // See storedValue.
  }
}
