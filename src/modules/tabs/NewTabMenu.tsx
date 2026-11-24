import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverAnchor,
  PopoverContent,
} from "@/components/ui/popover";
import { AgentLauncherPanel } from "@/modules/agents/components/AgentLauncherPanel";
import type { AgentLaunchRequest } from "@/modules/agents/lib/launcher";
import { usePreferencesStore } from "@/modules/settings/preferences";
import { useShortcutHint } from "@/modules/shortcuts";
import {
  defaultShellPath,
  type ShellInfo,
  useShells,
} from "@/modules/terminal/lib/shells";
import { useWorkspaceEnvStore } from "@/modules/workspace";
import {
  AiBrowserIcon,
  ArrowRight01Icon,
  ComputerTerminal02Icon,
  GitBranchIcon,
  Globe02Icon,
  IncognitoIcon,
  PencilEdit02Icon,
  PlusSignIcon,
} from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { useRef, useState } from "react";

/** A shortcut, set apart from its label so neither is cut at the edge. */
const HINT =
  "ml-6 shrink-0 whitespace-nowrap text-[11px] tabular-nums tracking-wide text-muted-foreground/80";

type Props = {
  onNew: () => void;
  onNewBlock: () => void;
  onNewPrivate: () => void;
  /** Opens a terminal tab with the given shell instead of the default. */
  onNewWithShell?: (shell: string) => void;
  onNewPreview: () => void;
  onNewEditor: () => void;
  onNewGitGraph: () => void;
  onLaunchAgents: (request: AgentLaunchRequest) => void;
};

export function NewTabMenu({
  onNew,
  onNewBlock,
  onNewPrivate,
  onNewWithShell,
  onNewPreview,
  onNewEditor,
  onNewGitGraph,
  onLaunchAgents,
}: Props) {
  const terminalHint = useShortcutHint("tab.new");
  const blockHint = useShortcutHint("tab.newBlock");
  const privateHint = useShortcutHint("tab.newPrivate");
  const editorHint = useShortcutHint("tab.newEditor");
  const previewHint = useShortcutHint("tab.newPreview");
  const shells = useShells();
  const shellSetting = usePreferencesStore((s) => s.terminalShell);
  // WSL and SSH spaces start their own login shell; this machine's shells
  // only apply to a local space.
  const local = useWorkspaceEnvStore((s) => s.env.kind === "local");
  const [menuOpen, setMenuOpen] = useState(false);
  const [launcherOpen, setLauncherOpen] = useState(false);
  const openLauncherAfterMenuClose = useRef(false);
  const openMenuAfterLauncherClose = useRef(false);

  const onMenuOpenChange = (next: boolean) => {
    if (next) {
      openLauncherAfterMenuClose.current = false;
      setLauncherOpen(false);
    }
    setMenuOpen(next);
  };

  const openLauncher = () => {
    openLauncherAfterMenuClose.current = true;
  };

  const backToMenu = () => {
    openMenuAfterLauncherClose.current = true;
    setLauncherOpen(false);
  };

  return (
    <Popover open={launcherOpen} onOpenChange={setLauncherOpen}>
      <PopoverAnchor asChild>
        <span className="inline-flex">
          <DropdownMenu open={menuOpen} onOpenChange={onMenuOpenChange}>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="ml-1 size-6 shrink-0 rounded-full bg-foreground/[0.06] text-muted-foreground ring-1 ring-inset ring-foreground/[0.04] hover:bg-foreground/[0.12] hover:text-foreground"
                title="New tab"
              >
                <HugeiconsIcon icon={PlusSignIcon} size={14} strokeWidth={2} />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="start"
              className="min-w-60"
              onCloseAutoFocus={(event) => {
                event.preventDefault();
                if (!openLauncherAfterMenuClose.current) return;

                openLauncherAfterMenuClose.current = false;
                requestAnimationFrame(() => setLauncherOpen(true));
              }}
            >
              {onNewWithShell && local && shells.length > 1 ? (
                <TerminalItemWithShells
                  hint={terminalHint}
                  shells={shells}
                  defaultPath={defaultShellPath(shells, shellSetting)}
                  onDefault={() => {
                    setMenuOpen(false);
                    onNew();
                  }}
                  onPick={onNewWithShell}
                />
              ) : (
                <DropdownMenuItem onSelect={onNew}>
                  <TerminalLabel hint={terminalHint} />
                </DropdownMenuItem>
              )}
              <DropdownMenuItem onSelect={onNewBlock}>
                <HugeiconsIcon
                  icon={ComputerTerminal02Icon}
                  size={14}
                  strokeWidth={1.75}
                  className="size-3.5 shrink-0 text-muted-foreground"
                />
                <span className="min-w-0 flex-1 truncate">Blocks</span>
                <span className={HINT}>{blockHint}</span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={openLauncher}>
                <HugeiconsIcon
                  icon={AiBrowserIcon}
                  size={14}
                  strokeWidth={1.75}
                  className="size-3.5 shrink-0 text-muted-foreground"
                />
                <span className="min-w-0 flex-1 truncate">Agents</span>
                <HugeiconsIcon
                  icon={ArrowRight01Icon}
                  size={14}
                  strokeWidth={1.75}
                  className="ml-4 size-3.5 shrink-0 text-muted-foreground"
                />
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onNewPrivate}>
                <HugeiconsIcon
                  icon={IncognitoIcon}
                  size={14}
                  strokeWidth={1.75}
                  className="size-3.5 shrink-0 text-muted-foreground"
                />
                <span className="min-w-0 flex-1 truncate">Privacy</span>
                <span className={HINT}>{privateHint}</span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onNewEditor}>
                <HugeiconsIcon
                  icon={PencilEdit02Icon}
                  size={14}
                  strokeWidth={1.75}
                  className="size-3.5 shrink-0 text-muted-foreground"
                />
                <span className="min-w-0 flex-1 truncate">Editor</span>
                <span className={HINT}>{editorHint}</span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onNewPreview}>
                <HugeiconsIcon
                  icon={Globe02Icon}
                  size={14}
                  strokeWidth={1.75}
                  className="size-3.5 shrink-0 text-muted-foreground"
                />
                <span className="min-w-0 flex-1 truncate">Preview</span>
                <span className={HINT}>{previewHint}</span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onNewGitGraph}>
                <HugeiconsIcon
                  icon={GitBranchIcon}
                  size={14}
                  strokeWidth={1.75}
                  className="size-3.5 shrink-0 text-muted-foreground"
                />
                <span className="min-w-0 flex-1 truncate">Git Graph</span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </span>
      </PopoverAnchor>
      <PopoverContent
        align="start"
        sideOffset={6}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (!openMenuAfterLauncherClose.current) return;

          openMenuAfterLauncherClose.current = false;
          requestAnimationFrame(() => setMenuOpen(true));
        }}
        className="w-[340px] gap-0 overflow-hidden rounded-2xl p-1.5"
      >
        <AgentLauncherPanel
          onBack={backToMenu}
          onLaunch={(request) => {
            setLauncherOpen(false);
            onLaunchAgents(request);
          }}
        />
      </PopoverContent>
    </Popover>
  );
}

function TerminalLabel({ hint }: { hint: string | null | undefined }) {
  return (
    <>
      <HugeiconsIcon
        icon={ComputerTerminal02Icon}
        size={14}
        strokeWidth={1.75}
        className="size-3.5 shrink-0 text-muted-foreground"
      />
      <span className="min-w-0 flex-1 truncate">Terminal</span>
      {hint ? <span className={HINT}>{hint}</span> : null}
    </>
  );
}

/**
 * The one Terminal row: a click (or Enter) opens the default shell, as
 * Ctrl+T does; hovering it, or the right arrow, lists this machine's
 * shells for a tab with another one.
 */
export function TerminalItemWithShells({
  hint,
  shells,
  defaultPath,
  onDefault,
  onPick,
}: {
  hint: string | null | undefined;
  shells: ShellInfo[];
  defaultPath: string | null;
  onDefault: () => void;
  onPick: (shell: string) => void;
}) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger
        onClick={(event) => {
          // A sub trigger only opens its list on click; this one opens a tab.
          event.preventDefault();
          onDefault();
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onDefault();
          }
        }}
      >
        <TerminalLabel hint={hint} />
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="min-w-56">
        {shells.map((shell) => (
          <DropdownMenuItem
            key={shell.path}
            title={shell.path}
            onSelect={() => onPick(shell.path)}
          >
            <span className="min-w-0 flex-1 truncate">{shell.name}</span>
            {shell.path === defaultPath ? (
              <span className={HINT}>Default</span>
            ) : !shell.integrated ? (
              <span className={HINT}>No blocks</span>
            ) : null}
          </DropdownMenuItem>
        ))}
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}
