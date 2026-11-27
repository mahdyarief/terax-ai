import "@/styles/globals.css";

import { lazy, Suspense, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { fetchDevice, type Device } from "./api";
import { PairingScreen, takePairCode } from "./PairingScreen";

// The terminal renderer and its WASM are heavy; the pairing screen must not
// wait for them.
const RemoteApp = lazy(() => import("./RemoteApp"));

type State =
  | { status: "checking" }
  | { status: "unpaired" }
  | { status: "paired"; device: Device };

function applyTheme(): void {
  try {
    const stored = localStorage.getItem("terax-ui-theme-shadow");
    const dark =
      stored === "dark" ||
      (stored !== "light" &&
        window.matchMedia("(prefers-color-scheme: dark)").matches);
    document.documentElement.classList.add(dark ? "dark" : "light");
    document.documentElement.style.backgroundColor = dark
      ? "#141414"
      : "#ffffff";
  } catch {
    document.documentElement.classList.add("dark");
  }
}

function WebRoot() {
  const [state, setState] = useState<State>({ status: "checking" });

  useEffect(() => {
    let live = true;
    void fetchDevice()
      .then((device) => {
        if (!live) return;
        setState(
          device ? { status: "paired", device } : { status: "unpaired" },
        );
      })
      .catch(() => {
        if (live) setState({ status: "unpaired" });
      });
    return () => {
      live = false;
    };
  }, []);

  if (state.status === "checking") {
    return (
      <p className="flex min-h-dvh items-center justify-center bg-background text-sm text-muted-foreground">
        Connecting…
      </p>
    );
  }
  if (state.status === "unpaired") {
    return (
      <PairingScreen
        initialCode={takePairCode()}
        onPaired={(device) => setState({ status: "paired", device })}
      />
    );
  }
  return (
    <Suspense
      fallback={
        <p className="flex min-h-dvh items-center justify-center bg-background text-sm text-muted-foreground">
          Loading your terminals…
        </p>
      }
    >
      <RemoteApp device={state.device} />
    </Suspense>
  );
}

applyTheme();
createRoot(document.getElementById("web-root") as HTMLElement).render(
  <WebRoot />,
);
