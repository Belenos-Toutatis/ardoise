/**
 * One IntersectionObserver for every card of the grid (a thousand notes would otherwise make a
 * thousand observers): `whenNearViewport` calls back once, when the element comes within
 * `NEAR` pixels of the viewport.
 */
const NEAR = '300px';

let shared: IntersectionObserver | null = null;
const waiting = new WeakMap<Element, () => void>();

/**
 * Calls `ready` once `element` is near the viewport (at once without IntersectionObserver).
 * Returns the function that stops waiting.
 */
export function whenNearViewport(element: Element, ready: () => void): () => void {
  if (typeof IntersectionObserver === 'undefined') {
    ready();
    return () => {};
  }
  shared ??= new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        const callback = waiting.get(entry.target);
        waiting.delete(entry.target);
        shared?.unobserve(entry.target);
        callback?.();
      }
    },
    { rootMargin: NEAR },
  );
  waiting.set(element, ready);
  shared.observe(element);
  return () => {
    waiting.delete(element);
    shared?.unobserve(element);
  };
}
