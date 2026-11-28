---
title: "The bridge decides a request is local from the connection's address, never from the Host header"
author: Mahdy
date: 2026-11-28
branch: dydev
cost: { minutes: 5, tokens: 9286 }
id: l-8bff58cf
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/modules/web_remote/server.rs
  - src-tauri/src/modules/web_remote/app/invoke.rs
---

## Decided
- `accept_loop` attaches `PeerAddr` (the socket peer) to every request.
- `transport_for` gives Loopback only when that peer IP is loopback, including `::ffff:127.0.0.1`. With no peer it gives PlainRemote. In https mode it gives Https.

## Tried and ruled out
- Reading loopback from the `Host` header, as first implemented. The tailscale and all-interfaces host allowlists accept `127.0.0.1` and `localhost`, so a device on the tailnet or LAN could send `Host: 127.0.0.1:7681` and run Secret-class commands (secrets_set, secrets_has) over plain HTTP.

## Watch out
- Code that calls `respond` directly, without `accept_loop`, has no `PeerAddr`, so its requests are treated as remote.
- `deps.https` still marks every request Https, including plain requests to the tailnet IP while `tailscale serve` is on. That traffic is WireGuard-encrypted, but it is not browser HTTPS.

In the code: src-tauri/src/modules/web_remote/server.rs (`PeerAddr`, `accept_loop`), src-tauri/src/modules/web_remote/app/invoke.rs (`transport_for`, `transport_from`, test `loopback_comes_from_the_peer_address_never_the_host_header`)
