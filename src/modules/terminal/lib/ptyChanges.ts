type Listener = () => void;

const listeners = new Set<Listener>();

/**
 * Tells listeners that a terminal pane gained or lost its PTY. The pane's
 * PTY id is read through `ptyIdForLeaf`; this only says it may have changed,
 * so a listener that mirrors pane → PTY (the remote tab list) looks again.
 */
export function notifyPtyChange(): void {
  for (const listener of listeners) listener();
}

export function subscribePtyChanges(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
