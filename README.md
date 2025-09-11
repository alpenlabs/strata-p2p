# Strata P2P

Peer‑to‑peer (P2P) stack for Strata‑based systems.

This library provides a modular,
feature‑gated LibP2P implementation that supports pub/sub (gossipsub),
direct request/response,
QUIC/TCP transports,
and optional Kademlia DHT.
It can run either with a built‑in signer (non‑BYOS)
or with an application‑provided signer (BYOS).

## v2 at a glance

- Modular architecture with clear feature flags.
- Signed envelopes for both gossip and request/response:
  - Your app passes/receives raw bytes; the library wraps and verifies a signed JSON envelope (protocol/version/pubkey/timestamp/signature).
  - BYOS setup/handshake also uses JSON.
- BYOS mode (bring your own signer) with an explicit handshake that exchanges application public keys and enforces an application‑key allowlist.
- Request/response framing uses raw bytes at the transport layer; the content is the signed JSON envelope.
- Optional Kademlia DHT (cannot be combined with BYOS for now).
- Configurable identifiers: protocol_name for identify/request-response (default `"/strata"`), and gossipsub_topic (default `"strata"`).
- Tunables: `envelope_max_age` (drop stale envelopes; default 300s), `max_clock_skew` (reject future‑dated envelopes; default 0s), `gossipsub_max_transmit_size` (default 512 KiB), and logging via `RUST_LOG`.

## Feature flags

- `gossipsub` — pub/sub messaging.
- `request-response` — direct request/response messaging.
- `quic` — QUIC transport (enabled by default).
- `kad` — Kademlia DHT (server mode).
- `byos` — Bring Your Own Signer: application‑key handshake + allowlist; messages are signed with your provided signer.
- `kad` and `byos` cannot be enabled together (compile‑time guarded).

Default features: `quic`, `gossipsub`, `request-response`.

## Quickstart (non‑BYOS)

Non‑BYOS uses the transport keypair to sign messages transparently. Typical steps:
1. Generate a transport keypair (libp2p Ed25519).
1. Build `P2PConfig`:

   - Set `transport_keypair`.
   - Provide one or more `listening_addrs` (e.g., `/ip4/127.0.0.1/udp/0/quic-v1` or `/ip4/127.0.0.1/tcp/0`).
   - Optionally set `connect_to` peers (multiaddrs).
   - Optional timeouts, buffer sizes, and limits (`envelope_max_age`, `max_clock_skew`, `gossipsub_max_transmit_size`, `request_max_bytes`, `response_max_bytes`, `handle_default_timeout`).
   - Optional identifiers: override `protocol_name` (default `"/strata"`) and `gossipsub_topic` (default `"strata"`).

1. Construct a swarm with either the in‑memory or default transport helper (in‑memory if your listening address is `/memory/*`).
1. Build the `P2P` instance.
1. Get handles:

   - `CommandHandle` for query/connect/disconnect.
   - `GossipHandle` for pub/sub (if `gossipsub`).
   - `ReqRespHandle` for request/response (if `request-response`).

1. Spawn `p2p.listen()` in a task.
1. Send/receive:

   - Gossip: send raw bytes via the gossip handle; you’ll receive raw bytes back from events.
   - Req/Resp: send a request with raw bytes; handle incoming requests via events and reply with raw bytes using the provided one‑shot.

Notes:

- QUIC is preferred automatically if present in the address list; the dialer will fall back to TCP if QUIC fails (when both are provided).
- Envelopes older than envelope_max_age are dropped (default 300s). Gossip messages larger than `gossipsub_max_transmit_size` are rejected (default 512 KiB).
- Use `CommandHandle::is_connected` / `get_connected_peers` / `QueryP2PState::GetMyListeningAddresses` for runtime state.

## Quickstart (BYOS)

BYOS lets your app control signing with its own keys and enforces an application‑key allowlist.

1. Implement the `ApplicationSigner` trait:

   - Provide a `sign(&[u8]) -> [u8; 64]` backed by your app private key(s).

1. Build `P2PConfig`:

   - Set `app_public_key` (the public half of your app signing key).
   - Set `transport_keypair` (libp2p Ed25519).
   - Provide `listening_addrs` and optional `connect_to`.
   - Optional identifiers and limits: override `protocol_name` (default `"/strata"`) and `gossipsub_topic` (default `"strata"`); tune `envelope_max_age`, `max_clock_skew`, `gossipsub_max_transmit_size`, `request_max_bytes`, `response_max_bytes`.

1. Supply an application allowlist (`Vec` of application public keys) and your signer when calling `P2P::from_config`.
1. Start `p2p.listen()`; the setup handshake will exchange/verify application public keys and enforce the allowlist before allowing traffic.
1. Use gossip/request‑response handles as in non‑BYOS; the library will sign/verify envelopes using your signer and the peer’s app public key.

<!-- Notes:

- BYOS cannot be combined with Kademlia (`kad`) and is rejected at compile time if both features are enabled. -->

## Kademlia (DHT)

- Enable the `kad` feature to include a Kademlia behaviour (server mode) for discovery.
- Do not enable `kad` together with `byos` (mutually exclusive).
- `P2PConfig` exposes a `kad_protocol_name` option (defaults to v1).

Use cases:

- Dynamic peer discovery (non‑BYOS deployments).
- Combine with gossipsub and/or request‑response as needed.

## License

Dual‑licensed under Apache‑2.0 and MIT.
