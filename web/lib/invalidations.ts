import { hashKey, type QueryClient, type QueryKey } from "@tanstack/react-query";

// Collapses the queries a burst of notifications makes stale into one invalidation `delay`
// milliseconds after the first of them. A read in flight runs to its end rather than being
// cut short, so a read slower than the stream of notifications still finishes; it may have
// left before the change, so its query is invalidated again at the next moment. A query
// `held` names is only marked stale, and is read again when something else asks for it.
export function invalidationCollapser(client: QueryClient, delay: number, held: (queryKey: QueryKey) => boolean = () => false) {
  let pending = new Map<string, QueryKey>();
  let timer: ReturnType<typeof setTimeout> | null = null;
  function flush() {
    timer = null;
    const due = [...pending.values()];
    pending = new Map();
    // Which reads are in flight is taken before any invalidation starts a read of its own.
    const again = due.filter((queryKey) => !held(queryKey) && client.isFetching({ queryKey }) > 0);
    for (const queryKey of due) {
      if (held(queryKey)) client.invalidateQueries({ queryKey, refetchType: "none" });
      else client.invalidateQueries({ queryKey }, { cancelRefetch: false });
    }
    add(again);
  }
  // The first notification of a burst sets the moment; later ones join it rather than
  // pushing it out, so a steady stream still invalidates once per `delay`.
  function add(queryKeys: QueryKey[]) {
    if (!queryKeys.length) return;
    for (const queryKey of queryKeys) pending.set(hashKey(queryKey), queryKey);
    if (timer === null) timer = setTimeout(flush, delay);
  }
  return {
    add,
    cancel() {
      if (timer !== null) clearTimeout(timer);
    },
  };
}
