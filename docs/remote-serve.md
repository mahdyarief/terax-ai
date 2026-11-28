# Running Terax without a desktop session

`--remote-serve` starts Terax with remote access already on and the main
window hidden, so a machine nobody sits at still serves its terminals to a
browser. Pair a device from that browser: open `http://<host>:<port>/web-index.html`
with a pairing code. On start, `--remote-serve` prints one pairing URL to
stderr (`journalctl --user -u terax-remote` under systemd). Each code works
once and expires after 10 minutes. For another code, without a restart, send
the process `SIGUSR1`; it prints a new URL the same way:

```sh
systemctl --user kill -s USR1 terax-remote
journalctl --user -u terax-remote | grep "pair a browser" | tail -1
```

Up to 8 codes can be open at once, so several devices can pair; a new code
does not end the earlier ones. Five wrong codes in a row end every open code.
Desktops with a window can also create codes from Settings → Remote.

A new device may only watch. Without a window there is no Settings page to
grant typing or opening tabs, so edit the device store while the service is
stopped: in `~/.local/share/app.crynta.terax/remote-devices.json`, set the
device's `"grant": {"input": true, "tabs": true, "app": true}`, then start it
again. `app` lets the device open the full app at `/app/`.

```sh
terax --remote-serve --bind tailscale --port 7681
```

Flags:

| Flag | Meaning |
|---|---|
| `--bind loopback` | This computer only (the default). |
| `--bind tailscale` | This machine's `100.x` address, inside WireGuard. |
| `--bind all` | Every interface; plain HTTP there needs the Settings opt-in. |
| `--port N` | The listener's port (default 7681). |

Settings → Remote shows the address, the paired devices and their grants. The
same listener, started from Settings without the flag, behaves identically.

## Linux without a display

The webview needs an X server even though nothing is shown. `xvfb-run` gives it
one:

```sh
xvfb-run -a terax --remote-serve --bind tailscale --port 7681
```

Install the runtime dependencies first (the WebKitGTK the app links against is
not in a bare server image):

```sh
sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0
```

## systemd user unit

`~/.config/systemd/user/terax-remote.service`:

```ini
[Unit]
Description=Terax remote terminal server
After=network-online.target tailscaled.service
Wants=network-online.target

[Service]
Type=simple
Environment=DISPLAY=:99
ExecStart=/usr/bin/xvfb-run -a -s "-screen 0 1280x800x24" /usr/local/bin/terax --remote-serve --bind tailscale --port 7681
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
```

```sh
systemctl --user daemon-reload
systemctl --user enable --now terax-remote
loginctl enable-linger "$USER"   # keep it running after logout
```

## HTTPS

With `tailscale serve`, the browser gets a real certificate on 443. Turn it on
in Settings → Remote (bind must be Tailscale). Terax refuses to add to a serve
configuration it did not create, and removes its own mount on stop.
