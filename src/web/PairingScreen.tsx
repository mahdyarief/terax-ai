import { useState } from "react";
import { pairDevice, type Device } from "./api";

/** The fragment carries the one-time code, so a pasted link needs no typing. */
export function takePairCode(): string {
  const hash = window.location.hash;
  const match = /(?:^#|&)pair=([A-Za-z0-9]+)/.exec(hash);
  if (!match) return "";
  // The code must not stay in the address bar or the history entry.
  history.replaceState(
    null,
    "",
    window.location.pathname + window.location.search,
  );
  return match[1].toUpperCase();
}

type Props = {
  initialCode: string;
  onPaired: (device: Device) => void;
};

export function PairingScreen({ initialCode, onPaired }: Props) {
  const [code, setCode] = useState(initialCode);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      onPaired(await pairDevice(code.trim().toUpperCase(), name.trim()));
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
      setBusy(false);
    }
  };

  return (
    <main className="flex min-h-dvh items-center justify-center bg-background p-6 text-foreground">
      <form
        onSubmit={submit}
        className="flex w-full max-w-sm flex-col gap-4 rounded-xl border border-border bg-card p-6"
      >
        <div>
          <h1 className="text-lg font-semibold">Pair this device</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            Open Terax, choose Settings → Remote access, and start pairing. The
            code expires in ten minutes.
          </p>
        </div>

        <label className="flex flex-col gap-1 text-sm">
          Pairing code
          <input
            value={code}
            onChange={(event) => setCode(event.target.value)}
            autoComplete="one-time-code"
            autoCapitalize="characters"
            spellCheck={false}
            maxLength={6}
            required
            className="rounded-md border border-border bg-background px-3 py-2 font-mono text-base tracking-[0.3em] uppercase"
          />
        </label>

        <label className="flex flex-col gap-1 text-sm">
          Name this device
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="Phone, work laptop…"
            maxLength={60}
            required
            className="rounded-md border border-border bg-background px-3 py-2 text-base"
          />
        </label>

        {error ? (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        ) : null}

        <button
          type="submit"
          disabled={busy}
          className="rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground disabled:opacity-60"
        >
          {busy ? "Pairing…" : "Pair"}
        </button>
      </form>
    </main>
  );
}
