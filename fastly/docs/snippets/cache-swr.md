Sets the stale-while-revalidate period for the cached item, which is the time for which
the item can be safely used despite being considered stale.

Having a stale-while-revalidate period provides a signal that the cache should be updated
(or its contents otherwise revalidated for freshness) asynchronously, while the stale cached
item continues to be used, rather than blocking on updating the cached item. The methods
[`Found::is_usable`] and [`Found::is_stale`] can be used to determine the current state of
a found item.

The stale-while-revalidate period is `Duration::ZERO` by default.
