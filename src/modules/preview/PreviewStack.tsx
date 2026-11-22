import type { PreviewTab, Tab } from "@/modules/tabs";
import { slotOf, slotStyle } from "@/modules/tabs/lib/splitLayout";
import { useEffect, useRef } from "react";
import { PreviewPane, type PreviewPaneHandle } from "./PreviewPane";

type Props = {
  tabs: Tab[];
  activeId: number;
  /** Tab shown on the right of a split view, if any. */
  secondaryId?: number | null;
  onUrlChange: (id: number, url: string) => void;
  registerHandle: (id: number, handle: PreviewPaneHandle | null) => void;
};

export function PreviewStack({
  tabs,
  activeId,
  secondaryId = null,
  onUrlChange,
  registerHandle,
}: Props) {
  const previews = tabs.filter(
    (t): t is PreviewTab => t.kind === "preview" && !t.cold,
  );

  const registerRef = useRef(registerHandle);
  const urlChangeRef = useRef(onUrlChange);
  useEffect(() => {
    registerRef.current = registerHandle;
  }, [registerHandle]);
  useEffect(() => {
    urlChangeRef.current = onUrlChange;
  }, [onUrlChange]);

  const refCallbacks = useRef(
    new Map<number, (h: PreviewPaneHandle | null) => void>(),
  );
  const urlCallbacks = useRef(new Map<number, (url: string) => void>());

  const getRefCallback = (id: number) => {
    let cb = refCallbacks.current.get(id);
    if (!cb) {
      cb = (h: PreviewPaneHandle | null) => registerRef.current(id, h);
      refCallbacks.current.set(id, cb);
    }
    return cb;
  };
  const getUrlCallback = (id: number) => {
    let cb = urlCallbacks.current.get(id);
    if (!cb) {
      cb = (url: string) => urlChangeRef.current(id, url);
      urlCallbacks.current.set(id, cb);
    }
    return cb;
  };

  useEffect(() => {
    const live = new Set(previews.map((t) => t.id));
    for (const id of refCallbacks.current.keys()) {
      if (!live.has(id)) refCallbacks.current.delete(id);
    }
    for (const id of urlCallbacks.current.keys()) {
      if (!live.has(id)) urlCallbacks.current.delete(id);
    }
  }, [previews]);

  if (previews.length === 0) return null;
  const split = secondaryId !== null;
  return (
    <div className="relative h-full w-full">
      {previews.map((t) => {
        const slot = slotOf(t.id, activeId, secondaryId);
        const visible = slot !== null;
        return (
          <div
            key={t.id}
            className={
              visible ? "pointer-events-auto" : "invisible pointer-events-none"
            }
            style={slotStyle(slot, split)}
            aria-hidden={!visible}
          >
            <PreviewPane
              ref={getRefCallback(t.id)}
              url={t.url}
              visible={visible}
              onUrlChange={getUrlCallback(t.id)}
            />
          </div>
        );
      })}
    </div>
  );
}
