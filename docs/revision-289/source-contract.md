# Revision 289 source contract

Status: source-grounded offline contract, pending reviewer acceptance. This is not a live compatibility claim.

## Authority and provenance

Primary source is RuneWiki/openrs2-nonfree branch 289 at commit `0c00ef249546fada67b1f6eb8bbe01ea7c250c95`, entry `nonfree/client/src/main/java/client.java`. The source inventory and complete tracked-file hashes are recorded in the operator-supplied `deob-289-source-inventory.md` and `deob-289-source-provenance.json`.

Verified source hashes:

- `client.java`: `d03a34d8c965a426993f5a3e812bc566a80a18a91de65e994a257c49a5f5fea6`
- deob `nonfree/var/cache/deob/client.jar`: `3ec08f3e733f0f021d73f180300f00349cb01e4a50803ab5f9efe2af839f70f6`
- upstream `nonfree/lib/289.2005-01-17.jar`: `51593c692b5bb2087b1c508f3e6030c3a256d38192eecce515b6ea563d2bc058`

These are source/deob/JAR artifacts, not a verified host-loadable game-cache pairing. The intended cache manifest, compatible server endpoint, credentials/test authorization, and replay capture are absent. `vendor/client-java-289` at `6834c7255f559db5f1702b8b0e5e7286a7d61244` is corroboration only and is not promoted over the primary source.

## Framing and handshake

At `client.java:2553-2587`, the client reads one opcode byte, subtracts ISAAC when active, selects `Class17.anIntArray209[opcode]`, then handles fixed sizes, `-1` byte lengths, and `-2` big-endian `g2` lengths. It refuses to consume a frame until the declared payload is available. Player updates at `client.java:6150-6168` additionally require the final cursor to equal the declared packet size; this is a mandatory invariant for Rust.

Login at `client.java:8342-8398` sends request opcode 14 plus world/login-server seed, receives an eight-byte server seed, builds RSA input containing four ISAAC seed words, client signlink value, username and password, then sends revision `289`, cache/config indices and RSA payload. Response 2 enters the game; responses 3-14 and 16-21 are explicit error/retry outcomes. Response 15 clears packet/frame state without being equivalent to a successful logout. The RSA modulus/exponent values and operator-approved endpoint remain outside this public contract.

## Inbound contract

`protocol-289.json` enumerates all 256 decoded inbound IDs and their exact `Class17.anIntArray209` lengths. Named rows are source-derived for the relevant implementation cut: actor/player update (188), inventory full (172), inventory partial (76), region rebuild (55), varp/config, widgets/interface, logout and actor reset. Other rows retain explicit unknown field descriptions rather than guessed schemas.

Important encodings:

- `g2`: unsigned big-endian two-byte value.
- `gsmart`: one byte for values below 128; otherwise unsigned `g2 - 0x8000`.
- variable frame lengths: `g1` for `-1`, `g2` for `-2`.
- actor updates: bit-packed counts, movement and masks; implementation must consume exactly the declared frame.
- inventory: revision-specific `g2` container/count shape and smart slot/item values; do not carry 274 widths into 289.
- widgets: component IDs and newline-terminated strings are cache/interface data, not public ABI constants.

## Outbound contract

The 75 outbound IDs are the complete numeric set observed at `client.java` `method465(...)` call sites. Payload lengths and field order are intentionally marked unknown where the branch-dependent writes have not yet been independently traced. This is a gate, not a guessed mapping: implementation must not treat the rows with `length: "unknown"` as production-safe.

## Existing Rust mapping

- `crates/client/src/io/packet.rs`: primitive g1/g2/gsmart/bit/RSA/ISAAC operations; keep as low-level primitives and add bounded error paths before production use.
- `crates/client/src/io/server_prot.rs` and `client_prot.rs`: current 274 tables; preserve public constants/default construction and add an explicit revision-selected profile.
- `crates/client/src/client/client.rs`, `game_shell.rs`: production lifecycle and packet dispatch seams to bind to the revision profile.
- `crates/client/src/login_rsa.rs`: existing RSA boundary; compare byte-for-byte against the 289 login sequence before changing it.
- `crates/client/src/io/client_stream.rs`: transport/read buffering; frame availability must be checked before dispatch.
- `crates/client/src/core/world.rs` and render modules: preserve existing scene ownership and `scene_state == 1` last-FBO freeze.

## Explicit unknowns and gates

1. No authoritative 289 game-cache manifest or server/cache pairing is available.
2. No approved live endpoint or credentials/test authorization is available.
3. Complete outbound field/length tracing remains required before enabling actions.
4. Exact actor local-player index, region base/plane timing, widget IDs and config/object definitions require primary-source tracing plus cache evidence.
5. Login/RSA/ISAAC needs an offline capture or independently checked vector; compilation is not proof.

The accompanying fixtures are tiny public-safe byte cases with manually derived expected lengths/states. They are contract checks, not claims that the current Rust decoder passes them.
