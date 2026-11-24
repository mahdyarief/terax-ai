import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useState } from "react";
import {
  type AskpassAnswer,
  type AskpassQuestion,
  isHostKeyQuestion,
  useAskpassStore,
} from "./askpass";

/** OpenSSH's question for the server being connected, one at a time. */
export function AskpassDialog() {
  const current = useAskpassStore((s) => s.queue[0]?.question ?? null);
  const answer = useAskpassStore((s) => s.answer);
  if (!current) return null;
  // Keyed by question: nothing typed outlives the question it was for.
  return <Question key={current.id} question={current} answer={answer} />;
}

function Question({
  question,
  answer,
}: {
  question: AskpassQuestion;
  answer: (answer: AskpassAnswer | null) => void;
}) {
  const [secret, setSecret] = useState("");
  const { kind, host, prompt } = question;
  const cancel = () => {
    setSecret("");
    answer(null);
  };
  const reply = (text: string, confirmed = false) => {
    setSecret("");
    answer({ answer: text, confirmed });
  };

  let title = `SSH: ${host}`;
  let body: React.ReactNode = null;
  let actions: React.ReactNode;
  if (kind === "secret") {
    body = (
      <Input
        type="password"
        autoFocus
        autoComplete="off"
        spellCheck={false}
        aria-label="Answer"
        value={secret}
        onChange={(e) => setSecret(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            reply(secret);
          }
        }}
      />
    );
    actions = (
      <>
        <Button variant="ghost" onClick={cancel}>
          Cancel
        </Button>
        <Button onClick={() => reply(secret)}>Continue</Button>
      </>
    );
  } else if (kind === "changed_host_key") {
    title = `The host key of ${host} has changed`;
    actions = (
      <Button variant="ghost" onClick={cancel} autoFocus>
        Close
      </Button>
    );
  } else if (kind === "confirm") {
    actions = (
      <>
        <Button variant="ghost" onClick={() => reply("", false)}>
          Deny
        </Button>
        <Button onClick={() => reply("", true)} autoFocus>
          Allow
        </Button>
      </>
    );
  } else if (kind === "notice") {
    actions = (
      <Button variant="ghost" onClick={() => reply("")} autoFocus>
        OK
      </Button>
    );
  } else {
    const hostKey = isHostKeyQuestion(kind);
    if (hostKey) title = `Trust the host key of ${host}?`;
    actions = (
      <>
        <Button variant="ghost" onClick={() => reply("no")} autoFocus>
          {hostKey ? "Cancel" : "No"}
        </Button>
        <Button onClick={() => reply("yes")}>
          {hostKey ? "Trust and connect" : "Yes"}
        </Button>
      </>
    );
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) cancel();
      }}
    >
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription asChild>
            {/* OpenSSH's own text, verbatim: fingerprints must be exact. */}
            <pre className="select-text whitespace-pre-wrap break-words font-mono text-xs text-muted-foreground">
              {prompt.trim()}
            </pre>
          </DialogDescription>
        </DialogHeader>
        {kind === "changed_host_key" && (
          <div className="text-xs text-destructive">
            This can mean someone is intercepting the connection. Terax will not
            connect. If the server was reinstalled, remove its old key from
            known_hosts yourself.
          </div>
        )}
        {body}
        <DialogFooter>{actions}</DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
