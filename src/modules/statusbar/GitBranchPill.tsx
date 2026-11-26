import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from "@/components/ui/command";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Spinner } from "@/components/ui/spinner";
import { git } from "@/modules/git";
import type { GitBranchEntry } from "@/modules/git/types";
import type { SourceControlSummary } from "@/modules/source-control/useSourceControl";
import {
  Add01Icon,
  GitBranchIcon,
  SourceCodeIcon,
} from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import {
  gitBranchTitle,
  gitBranchView,
  newBranchName,
  switchableBranches,
} from "./lib/gitBranch";

type Props = {
  summary: SourceControlSummary;
  onOpenSourceControl: () => void;
};

/**
 * The repository's branch in the status bar, with commits to push and pull
 * and the number of changed files. Clicking it switches to another branch or
 * creates one from HEAD.
 */
export function GitBranchPill({ summary, onOpenSourceControl }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [branches, setBranches] = useState<GitBranchEntry[] | null>(null);
  const [busy, setBusy] = useState(false);
  const request = useRef(0);
  const repoRoot = summary.repo?.repoRoot ?? null;
  const { refresh } = summary;

  useEffect(() => {
    if (!open || !repoRoot) return;
    const id = ++request.current;
    setBranches(null);
    git
      .listBranches(repoRoot)
      .then((result) => {
        if (id === request.current) setBranches(result.branches);
      })
      .catch((e: unknown) => {
        if (id !== request.current) return;
        setBranches([]);
        toast.error(String(e));
      });
  }, [open, repoRoot]);

  const run = useCallback(
    async (action: (root: string) => Promise<unknown>) => {
      if (!repoRoot || busy) return;
      setBusy(true);
      try {
        await action(repoRoot);
        setOpen(false);
        setQuery("");
      } catch (e) {
        toast.error(String(e));
      } finally {
        setBusy(false);
        void refresh();
      }
    },
    [repoRoot, busy, refresh],
  );

  if (!summary.hasRepo || !summary.repo) return null;
  const view = gitBranchView({
    branch: summary.status?.branch ?? summary.repo.branch,
    isDetached: summary.status?.isDetached ?? summary.repo.isDetached,
    ahead: summary.ahead,
    behind: summary.behind,
    changed: summary.changedCount,
  });
  const title = gitBranchTitle(view, summary.upstream);
  const listed = branches ? switchableBranches(branches) : [];
  const create = branches ? newBranchName(query, branches) : null;

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setQuery("");
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          title={title}
          aria-label={title}
          className="terax-pill-in flex min-w-0 shrink cursor-pointer items-center gap-1 rounded px-1 text-[11px] font-medium tabular-nums text-muted-foreground outline-none transition-colors hover:bg-foreground/[0.06] hover:text-foreground focus-visible:ring-1 focus-visible:ring-primary/40"
        >
          {busy ? (
            <Spinner className="size-3" />
          ) : (
            <HugeiconsIcon icon={GitBranchIcon} size={12} strokeWidth={2} />
          )}
          <span className="max-w-40 truncate">
            {view.label}
            {view.changed > 0 ? "*" : ""}
          </span>
          {view.ahead > 0 ? <span>↑{view.ahead}</span> : null}
          {view.behind > 0 ? <span>↓{view.behind}</span> : null}
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        className="w-72 gap-0 rounded-2xl p-0"
      >
        <Command className="rounded-2xl">
          <CommandInput
            value={query}
            onValueChange={setQuery}
            placeholder="Switch branch, or name a new one"
          />
          <CommandList className="max-h-72">
            {branches === null ? (
              <div className="flex justify-center py-4">
                <Spinner className="size-4" />
              </div>
            ) : (
              <>
                <CommandEmpty>No branch matches.</CommandEmpty>
                {create ? (
                  <CommandGroup>
                    <CommandItem
                      value={`create ${create}`}
                      disabled={busy}
                      onSelect={() =>
                        void run(async (root) => {
                          await git.branchCreate(root, create);
                          await git.checkoutBranch(root, create);
                        })
                      }
                      className="text-xs"
                    >
                      <HugeiconsIcon icon={Add01Icon} size={13} />
                      <span className="truncate">
                        Create branch <strong>{create}</strong> from HEAD
                      </span>
                    </CommandItem>
                  </CommandGroup>
                ) : null}
                <CommandGroup heading="Branches">
                  {listed.map((b) => (
                    <CommandItem
                      key={b.name}
                      value={b.name}
                      data-checked={b.isHead}
                      disabled={busy || b.isHead}
                      onSelect={() =>
                        void run((root) => git.checkoutBranch(root, b.name))
                      }
                      className="text-xs"
                    >
                      <HugeiconsIcon icon={GitBranchIcon} size={13} />
                      <span className="truncate">{b.name}</span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}
            <CommandSeparator />
            <CommandGroup>
              <CommandItem
                value="open source control"
                onSelect={() => {
                  setOpen(false);
                  onOpenSourceControl();
                }}
                className="text-xs"
              >
                <HugeiconsIcon icon={SourceCodeIcon} size={13} />
                Open Source Control
              </CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
