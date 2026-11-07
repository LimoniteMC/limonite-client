# Status

A living page: where the project is, how to build and run it, and what bites. Update it whenever a
milestone step lands. Architecture decisions live in `docs/PLAN.md`; this page is about progress
and environment.

_Last updated: 2026-11-07._

## Where we are

- **Current milestone:** 5, playable singleplayer, under way: `docs/milestones/m5-plan.md`,
  approved on 2026-10-24 with the owner's five amendments.
  - **Step 1 is done (2026-10-26):** the official client's play packets, captured. The raw
    capture is in `run/captures/m5-official/` (gitignored).
    - **Its Results** in the plan are the spec for every packet the client sends.
    - **The fixtures:** 23 codec fixtures, in the fork (committed with Step 2), and 12 physics
      traces in `client/src/player/fixtures/`.
    - **Its Deviations** (1 to 8) record the script's changes and the upstream sync.
  - **Steps 2 and 3 are done (2026-10-26):** the fork's client codecs read 24 play packets and
    write the player's 12 actions, each written packet golden-tested against the official client.
    The fork commits are `77e0ac2`, `12ac8ba`, `2f4dff2` and `15745d6`, and the bump is `3429125`.
    - **The private golden tests** (commands and chat) read the M5 capture through
      `LIMONITE_M5_CAPTURES` (Dev environment, "Lint the fork").
  - **Step 4 is done (2026-10-26):** the home folder (`run/home`) holds `settings.toml` and this
    install's identity, `identity.toml`; the keymap binds keys, mouse buttons and the wheel by
    name; and the host is an operator of its worlds, with spawn protection off. Commits `2fe3859`
    and `09cdbc3`. The headless run has a sixth check, "operator".
  - **Step 5 is done (2026-10-26):** `Game` holds the world, the player and the time of day, and
    the player moves once a tick, drawn between ticks (`5b0f117`). The headless run has a seventh
    check, "teleport".
  - **Step 6 is done (2026-10-27):** the player walks, swims, climbs and flies under physics, and
    all 12 of the official client's traces replay within 0.000015 blocks, flag for flag (`895c7c5`,
    `3dd914f`). Where Bedrock's rules differ from Java's is m5-plan Deviation 17.
  - **Step 7 is done (2026-10-27):** the blocks M4 drew as stand-in cubes are drawn as Pumpkin's
    outline boxes, the same boxes physics collides with (`9a6fdf1`). Every Bedrock id's Java
    states have the same boxes, or one of the reasons in m5-plan Results, Step 7.
  - **Checkpoint 1 (2026-10-30):** the owner's walk found the physics right and the special models
    broken: dripstone, doors, leaf litter, chests and more. For many blocks `blocks.json`'s slots
    are a bag of textures picked by state, and Pumpkin's outline boxes aren't models.
  - **Step 7b is done (2026-10-30):** added at the checkpoint (m5-plan Deviation 23). Each block's
    textures are picked by its state (`blocks/rules.rs`), and 60-odd blocks have hand-made models
    (`blocks/models.rs`). Tests check every block Pumpkin can send, and every state of each model
    family against Pumpkin's outlines.
  - **Step 7c is done (2026-10-31):** the owner's idea (m5-plan Deviation 26). With the user's own
    Java jar, every block's real model is compiled at startup, in memory (`blocks/java_models.rs`),
    and drawn in Bedrock's textures where they fit. All 24,714 quads of 2,519 models agree with what
    Blockbench builds from the same files. Without a jar, Step 7b's models draw, the same to the byte.
    Nothing derived from a jar goes in the repo (CLAUDE.md, Rules).
  - **Step 7d is done (2026-10-31):** flipbooks animate (`render/animation.rs`), fluids slope and flow,
    and `--debug-world` holds every block state (`debug_world.rs`).
  - **Step 8 is done (2026-11-01):** the client's own UI (`client/src/ui/`), and the HUD on it: the crosshair,
    the hotbar, hearts, hunger and air, in the install's own sprites and font.
  - **Step 9 is done (2026-11-01):** the hotbar shows what the player carries (`client/src/inventory/`), with
    block icons from the world's own mesher (`render/icons.rs`), counts and the held item's name. 1 to 9 and the
    wheel select, and Q drops, byte for byte as the official client does. Picking waits for Step 10, and armour
    for Step 12.
  - **Fixed (2026-11-01, the owner's report):** walls gave way along any axis from about 256 blocks out from the
    origin, because a position's rounding could carry the box inside them (`ad5e250`; m5-plan Deviation 40, and
    Results, "The walls that gave way").
  - **Step 10 is done (2026-11-01):** a ray from the eyes finds the block aimed at, within Pumpkin's reach
    (`client/src/edit/aim.rs`), with an outline along its shape (`render/outline.rs`). A middle click sends the
    official client's pick, which Pumpkin ignores for now.
  - **Step 11 is built (2026-11-02):** blocks break and place, with the official client's packets, and show at once
    where the result is certain (`client/src/edit/`). A table takes back what the server refuses, and `--headless
    --edits <n>` checks it all against the real server.
  - **The owner's checkpoint (2026-11-02): go, on 2026-11-03.** The crack looks great. The client now draws the
    experience bar, so the HUD sits together, and a wider outline (m5-plan Deviations 49 and 50).
  - **The fork is on Bedrock 1.26.60 (2026-11-04),** protocol 2225, which the official client here speaks (m5-plan,
    Deviation 51).
  - **The official client joined** the stock server over NetherNet on 2026-11-05, on 1.26.60.5.
  - **Step 12 is done (2026-11-06):** the inventory screen, in survival and creative
    (`client/src/ui/screens/inventory.rs`), and an item stack request for each move, shown at once and confirmed or
    undone by the server (`client/src/inventory/requests.rs`). Each request is the official client's on 1.26.60, byte
    for byte, from the capture of 2026-11-06, and `--headless --inventory` checks the moves against the real server.
    The armour and the off hand show, now that Pumpkin sends them, and the HUD counts what's worn.
  - **Step 13 is done (2026-11-07):** the client works out sky and block light itself, by Pumpkin's own rules
    (`client/src/world/light/`), and meshing waits for it. `--headless` checks it against the server's light at every
    block, and explains each difference as a place where the server's light breaks its own rules (m5-plan Deviation
    57). Nothing draws it yet.
  - **Next:** Step 14, drawing the light.
  - **Found by the capture:** three Pumpkin bugs that hit every Bedrock player, the host included.
    They're reported upstream ("Upstream PR candidates").
  - **Checkpoints** after Steps 7, 11, 14 and 16 are go/no-go points: if a phase overruns, the
    owner can tick what's done and split the rest into its own milestone.
  - **Next:** milestone 5b, containers, added to PLAN.md at M5's approval; then M6.
- **Upstream syncs are weekly** (owner, 2026-10-26): merge upstream Pumpkin's `master` into the
  fork once a calendar week. `docs/upstream.md` has the procedure, and a log of what each sync
  brought in.
  - **Last sync:** 2026-11-04, upstream `a83abee` (64 commits, with the move to Bedrock 1.26.60). It left the fork's
    `limonite` at `8acb34b`, which `main` bumped in `010e373`. Step 12's codecs are on top, at `13bda25`, bumped in `483a043`.
  - **Next sync due:** 2026-11-11.
- **Milestone 4 is done (2026-10-24).** Plan, results and deviations: `docs/milestones/m4-plan.md`.
  - **What it built:** a window (winit) with a wgpu device and a fly camera that flies the host;
    CPU meshes for every loaded sub-chunk, drawn with vanilla textures read at runtime from the
    user's Bedrock install through the new `packs` crate; chunk unloading; and `--bench`, the
    frame-time baseline M6 must beat.
  - **The owner accepted it:** a flight in a fresh world, then a side-by-side with Bedrock 1.26.52
    at the same pose: a match, apart from the planned differences, once bamboo had its own shape.
  - **Owner decisions in M4:**
    - 2026-10-23: `blocks.json` merges per block, then per field (m4-plan Deviation 1);
    - 2026-10-24: the client crate builds at `opt-level = 1` in dev builds, so meshing keeps up
      after a join (Deviation 13). CPU meshing gets no further work ("Notes for M6");
    - 2026-10-24: bamboo gets its own shape (Deviation 14), and a census of the other stand-in
      cubes becomes the to-do list under "Special shapes to do".
- **Fork changes in M4:** a client-side reader for `NetworkChunkPublisherUpdate`, and one
  deliberate non-seam edit: a `Weather::downfall()` accessor in `pumpkin-util/src/biome.rs`, an
  upstream candidate.
- **Pushed, all public, with the owner's OK** (M1 and M2 on 2026-10-20, M3 on 2026-10-22, M4 on
  2026-10-24, the README and the wiki on 2026-10-25, the first upstream sync and M5 through Step 10
  on 2026-11-01, the second sync and Step 11 on 2026-11-02, Step 11's checkpoint on 2026-11-03, the move to
  Bedrock 1.26.60 on 2026-11-05, Step 12 on 2026-11-06, and Step 13 on 2026-11-07):
  - superproject `main` to `https://github.com/LimoniteMC/limonite-client` (`origin`; `main`
    tracks `origin/main`);
  - the fork's `limonite` branch, at `13bda25`, with the M1 to M4 fork commits, the upstream syncs
    and M5's play codecs. It tracks `origin/limonite`. The fork goes first, since `main`'s submodule
    pointer names its commit;
  - the wiki, `https://github.com/LimoniteMC/limonite-client.wiki.git` (`master`), from its
    clone in `run/wiki`. It goes after `main`, since its pages name `main`'s commit;
  - the org's page, `https://github.com/LimoniteMC/.github` (`main`), from its clone in
    `run/org-profile`. It goes last, since it claims what `main` has and links to its images.
- **No upstream PR is open.** The candidates are listed under "Upstream PR candidates". Step 9 added one, a
  Bedrock player's armour slots and off hand, which upstream's port to 1.26.60 has since sent (m5-plan Deviation
  52). Step 10 adds another: `get_block_outline_shapes_at` moves the boxes of grass, ferns, tall flowers and roots,
  which Java doesn't move, by as much as half a block (m5-plan Results, Step 10).

  Ask the owner before pushing anything, and before opening an upstream PR.
- **Where the repos are:** the owner moved both to the `LimoniteMC` org by 2026-11-01. `.gitmodules`
  and every `origin` point there, the wiki's too. The old `Doodle-Studios` and `LaDoodleInTheHat` URLs
  redirect, but don't use them. Older plans still name them where they record what happened then.
- **GitHub milestones (2026-11-01):** every milestone in PLAN.md has one on
  `LimoniteMC/limonite-client`, with M5's steps as a checklist. Keeping them current: Dev environment,
  "The GitHub milestones".
- **README and wiki (2026-10-25):** the README is rewritten for GitHub, with the owner's lemon
  added to the title, and the docs are mirrored to the GitHub wiki. Keeping both current: Dev
  environment, "The README and the wiki".

## Done

| Milestone | Result | Commits |
| --- | --- | --- |
| 0. Recon | Official Bedrock 1.26.52 joined stock Pumpkin (fork at `aac0a82`) over NetherNet; note in `docs/recon/m0-pumpkin-bedrock.md`; PLAN.md corrected | `main`: `54a51e6` scaffold, `341cdfb` recon note, `1f86cbe` PLAN.md corrections |
| 1. Workspace + fork seam | Cargo workspace with `client/`; fork as a submodule; `Transport` trait with NetherNet behind it; Java listener behind `java-listener`; `tracing` in the client. Bedrock 1.26.52 joined a `--no-default-features` server at `80b19bb` with `java.enabled = true` and no TCP 25565 listener | `main`: `75ae93d` plan, `c27b3e4` workspace, `8b083d9` submodule, `7ed6d8e` tracing, `7ff616c` profile cleanup, `f8971b4` bump. Fork `limonite`: `df1f8e524` Transport, `80b19bb9b` java-listener |
| 2. Headless embedded client | The client runs Pumpkin on a background thread and joins it as the host over an in-memory `LocalChannel`: unsigned-token login, spawn, 329 chunks, an 8-block walk the server saw (7.8), and no socket owned by the process. Client codecs in `pumpkin-protocol` behind `client`, golden-tested byte for byte against official-client captures. NetherNet regression join passed. `LocalChannel` cost measured; chunks dominate, and the `Arc` decision is deferred to M3's planning | `main`: `b6e34c9` plan, `975b261` bump + deps, `d05e0e1` embedded server, `90e4f22` login and spawn, `441b7a1` walk + check, `b68d672` measure, `7d372f3` bump, `d4f045d` block-actor timing, `3d31577` packet names, then the tick. Fork `limonite`: `de0f7cedc` accept_bedrock, `7c1fa8063` LocalChannel, `85f58b5f6` client codecs, `03b498c47` NBT test fix |
| 3. Chunk map | The client decodes every `LevelChunk` into Bedrock-native world state and applies `UpdateBlock`. The decoded world equals the server's `ChunkData` block for block on both layers, and biome for biome: 329 chunks in the dev world, 3,725 at radius 32. `--map` writes a top-down map, which the owner judged a match for Bedrock 1.26.52 at the same seed and position (stock fork server over NetherNet, also the regression join). The fork's palette conversion is 4.8× faster (encode 696 → 145 µs per chunk), byte-identical. The `Arc` decision: bytes stay | `main`: `ee42446` plan, `e6c2d3e` bump + deps, `da890ae` decoder, `b2eff5b` world check, `dedeba9` map, `7804524` measure, `be0ef05` progress, `133bc94` drift vs decoder bug, `e904911` bump, `04f05ae` keep the host alive, `e0a5858` results, `55d0c36` Arc decision, then the tick. Fork `limonite`: `dc3a0ac2e` UpdateBlock reader, `e251ffbda` palette speedup |
| 4. First pixels | A window with a fly camera that flies the host, drawing every loaded sub-chunk with vanilla textures read at runtime from the user's install (55 vanilla packs, folders and `__brarchive` archives; 954 layers of 32×32). Block names from the fork's Bedrock palette; chunks the server's view has left are dropped. The owner accepted a flight in a fresh world and a side-by-side with Bedrock 1.26.52 at the same pose. M6 baseline in release: 2,794, 1,016 and 166 fps at radius 8, 16 and 32 | `main`: `02eabf1` bump, `cb6732e` packs crate, `508fcd7` install + `--headless`, `838c2a7` palette, `3a991cd` appearance, `c0cb67a` deviations, `50a2bcf` progress, `c254e45` unloading, `bddce75` mesher, `9f73aff` window, `d72703f` terrain, `b4cb2dc` dev opt-level, `fa01f0d` deviations, `5ccc2e5` progress, `34447e5` `--bench`, `545dc37` baseline, `4ea93d0` progress, `90397b3` bamboo, `3028183` census, then the tick. Fork `limonite`: `10826210f` publisher reader, `443b954b8` downfall |

Key facts from M0 (details in the recon note, which is required reading before touching the fork):
- **Transport:** upstream Bedrock is **NetherNet** (WebRTC), not RakNet.
- **Protocol:** **exactly 2225** is accepted: Bedrock 1.26.60, since the sync of 2026-11-04. Before that it was 2193,
  Bedrock 1.26.51, which 1.26.52 also spoke.
- **The seam:** `BedrockClient::session: Arc<NetherNetSession>`, which uses six methods.
- **LocalChannel:** carries uncompressed bytes. Compression only turns on via
  `RequestNetworkSettings`, so the host never sends it.
- **Offline login:** needs an unsigned token with a `cpk` key, plus 10 `ClientData` fields
  (recon §6).
- **Release blocker:** the build compiles Mojang's Java datapack into the binary. Tracked in
  PLAN.md Risks and Open questions.

Key facts from M1 (details and deviations in `docs/milestones/m1-plan.md`):
- **The seam:** `crates/pumpkin-core/src/net/bedrock/transport.rs` holds `trait Transport` and
  `impl Transport for NetherNetSession`. `BedrockClient::session` is now `Arc<dyn Transport>`.
- **Java listener:** `pumpkin-core` has an opt-in `java-listener` feature, and `lib.rs` gates the
  bind with `cfg!()`. The `pumpkin` binary has it on by default. The client depends on
  `pumpkin-core` without it.
- **Fork diff:** five files against `aac0a82`. Keep new fork code in new modules.

Key facts from M2 (details, results and deviations in `docs/milestones/m2-plan.md`):
- **The client's modules** (`client/src/`):
  - `embedded.rs`: the server thread and its config, built in code;
  - `login.rs`: the host's unsigned tokens;
  - `packets.rs`: id-to-name table and batch framing;
  - `session.rs`: the 20 Hz loop, the skeleton of the M4 main loop;
  - `checks.rs` and `measure.rs`.
- **The fork:**
  - `net/bedrock/local.rs` holds `LocalChannel`, and `PumpkinServer::accept_bedrock` is the
    shared accept path.
  - `pumpkin-protocol`'s `client` feature adds the client-side codecs: derives on the structs,
    and hand-written mirrors in `bedrock/client_codecs/`.
  - The fork diff is 32 files against `aac0a82`, and edits to existing files are small.
- **Framing on `LocalChannel`:** one packet per batch; no `0xFE` and no compression either way.
- **Golden fixtures:** in `client_codecs/fixtures/`, with provenance in its README.
  - The `SLogin` capture holds the owner's Xbox identity, so it lives only in
    `run/captures/slogin.bin` (gitignored).
  - Raw captures are in `run/captures/m2-official/`.

Key facts from M3 (details, results and deviations in `docs/milestones/m3-plan.md`):
- **New client modules:**
  - `world/` holds the world state: `storage.rs` (one palette storage), `chunk.rs`,
    `level_chunk.rs` (the decoder and its typed errors), and `mod.rs` (`World`, the block table).
  - `map.rs` draws the map.
- **The world check:** "world matches server" compares through independent code on each side.
  After one full pass it re-checks the chunks that differed. If a chunk still differs, a fresh
  encode of the server's copy tells server drift apart from a decoder bug.
- **The fork:** two commits in M3.
  - `dc3a0ac` derives a `CUpdateBlock` reader behind `client`.
  - `e251ffb` speeds up the palette conversion, touching only `palette.rs`, with the old code kept
    as a test oracle.
  - The fork diff is 34 files against `aac0a82`.
- **Measurements** (release; chunk cost in phases):
  - E, the encode: 145 µs per chunk;
  - C, the palette conversion: 130 µs;
  - D, our decode: 21 µs;
  - S, a snapshot: 8 µs.
  - A radius-32 join is 3,725 chunks in about 21 s.
- **Pumpkin bug:** its fire never tells clients about some changes (Upstream PR candidates).

Key facts from M4 (details, results and deviations in `docs/milestones/m4-plan.md`):
- **The `packs` crate** (`packs/`): `brarchive.rs` reads `__brarchive` archives with typed errors,
  `pack.rs` and `stack.rs` stack folder packs (`PackStack::vanilla` picks the vanilla packs), and
  `registry.rs` merges `blocks.json` and `terrain_texture.json`. PLAN.md, Resource packs, has
  the format and the rules.
- **New client modules:**
  - `install.rs` finds the Bedrock install and loads the vanilla stack;
  - `blocks/`: the palette (`palette_source.rs` is the only code that knows its source),
    appearance (shapes, textures, tints and render classes per Bedrock id), and the stand-in
    census;
  - `render/`: the device (`mod.rs`), camera, textures, tints, the mesher, the GPU meshes
    (`meshes.rs`), and the terrain pipelines and shader (`terrain.rs`, `terrain.wgsl`);
  - `app.rs` (the winit loop), `input.rs`, `settings.rs` and `bench.rs`.
- **The loop:** ticks are independent of frames. One `PlayerAuthInput` per 50 ms whatever the
  frame rate, so a minimised window stays connected.
- **Numbers:** a sub-chunk meshes in 144 µs in release and 585 µs in a dev build; a radius-8 view
  is about 1,937 meshes and 65 MB of vertex buffers. The palette parses in 35 to 43 ms in
  release.

## Dev environment

Windows 11, with Git Bash and PowerShell both used.

**Toolchain**
- rustc/cargo **1.99.1** stable (MSVC), with clippy.
- VS 2022 C toolchain and CMake are installed. Python 3.14 is there too, with Pillow, for throwaway
  scripts.

**Pumpkin fork: `pumpkin/`**
- A git submodule of `https://github.com/LimoniteMC/Pumpkin`, branch **`limonite`**, at
  `8acb34b`: the M1 to M4 commits on upstream `aac0a82`, with upstream `master` `5e9101f`
  (2026-10-26) merged in (`845450b`), then the NBT reader fix and M5's play codecs, upstream
  `eba538f` merged in (2026-11-02), and last upstream `a83abee`, with Bedrock 1.26.60 (`4caebaf`,
  2026-11-04). Its git directory is `.git/modules/pumpkin`.
- `upstream` points at `https://github.com/Pumpkin-MC/Pumpkin.git`, whose default branch is
  `master`. The fork merges it weekly (`docs/upstream.md`).
- **The fork's whole diff:** `git -C pumpkin diff upstream/master` (after a fetch) shows exactly
  what Limonite adds.
- **Fresh clone:** clone `https://github.com/LimoniteMC/limonite-client`, then set long
  paths before the submodule checks out. Run `git -c core.longpaths=true submodule update
  --init`, then `git -C pumpkin config core.longpaths true`.

**Run the client: the window** (run at the repo root):

```bash
cargo run -p limonite-client
```

- **What it does:** finds the Bedrock install, then loads the vanilla packs, the block palette,
  the appearance table and the textures. Each step logs one line, with counts and problems.
  Then it starts the embedded server, joins as the host, and opens the window.
  - **The install:** the GDK default, `<drive>:\XboxGames\Minecraft for Windows\Content`, or
    `--bedrock <path>` to the folder that holds `data/resource_packs`. Vanilla files are read at
    runtime and never copied.
- **Controls:** click in the window to capture the mouse, and move the mouse to look. W, A, S and
  D walk, Space jumps, Left Shift sneaks, and Left Ctrl sprints. In creative, a double tap on
  Space flies, Space and Shift rise and sink, and another double tap lands. Esc releases the mouse
  (until the pause menu, Step 16). P logs the pose as a `/tp` command for the official client.
  Every binding is in the settings file (below).
- **The title bar** shows the frame rate and frame time, chunks and those still waiting for light,
  meshes and the queue, visible meshes and draws, the position, and the sky and block light in the
  block at the player's feet. Closing the window saves the world and exits.
- **Window flags:**
  - `--bench <seconds>`: the M6 baseline flight ("Notes for M6"). Use a release build. It keeps
    M4's 70-degree field of view, whatever the settings say.
- **Dev builds** compile the client crate at `opt-level = 1` (root `Cargo.toml`), so meshing keeps
  up after a join. Overflow checks and debug assertions stay on.

**Screenshots with no window** (m5-plan Step 8): the real server, client and renderer, saving frames as
PNGs to `run/screenshots`, gitignored. `client/src/shots.rs` has the format of the list, and each line is one
picture: a name, a gamemode, health, hunger, a pose as the `P` key logs it, and seconds to wait. The owner's
copy of the list is in `run/step8`. The world is a copy, because the run changes it.

```bash
LIMONITE_SHOTS=run/step8/shots-world.txt LIMONITE_SHOTS_ARGS="--data-dir run/step8/shots-world --home run/step8/home --radius 16" LIMONITE_SHOTS_HOTBAR="dirt*64,oak_log*12,poppy,torch*32,oak_stairs*16,diamond_sword,apple*5,oak_fence*8,grass_block*64" CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo test --release -p limonite-client --bin limonite-client screenshots -- --ignored --nocapture
```

The HUD alone, every piece of it over stripes, goes to `run/sheets`, and the inventory screen with
`inventory_sheets` (`_sheets` runs both):

```bash
LIMONITE_BEDROCK="C:/XboxGames/Minecraft for Windows/Content" cargo test -p limonite-client --bin limonite-client hud_sheets -- --ignored --nocapture
```

**The debug world** (m5-plan Step 7d): every block state in a grid, and water and lava set out beside it,
in `run/debug-world`. `]` and `[` go to the next and last block, `.` and `,` to the next and last state, and
typing in the terminal goes to what matches, such as `oak_door` or `door upper`. `--goto <text>` starts there.

```bash
cargo run --release -p limonite-client -- --debug-world
```

**The Java jar** (m5-plan Step 7c): the client looks for `--java-jar <file>`, then
`%APPDATA%\.minecraft\versions\26.3\26.3.jar`, then `%APPDATA%\Blockbench\minecraft_assets_cache\26.3.jar`,
where the owner's is. It logs what it found. Tests and sheets take `LIMONITE_JAVA_JAR`.

```bash
LIMONITE_JAVA_JAR="$APPDATA/Blockbench/minecraft_assets_cache/26.3.jar" LIMONITE_BEDROCK="C:/XboxGames/Minecraft for Windows/Content" cargo test -p limonite-client --bin limonite-client -- --ignored --skip contact_sheets --skip dump_every --nocapture
```

- **What those print:** every Java state against the jar, against Pumpkin's outlines, and against the
  states it shares a Bedrock id with, in names and counts.
- **Comparing with Blockbench,** after a version bump. It starts Blockbench with a throwaway profile,
  and leaves your own alone. `run/` is gitignored, and this must stay there, as it's derived from
  Mojang's files:

  ```bash
  LIMONITE_JAVA_JAR="$APPDATA/Blockbench/minecraft_assets_cache/26.3.jar" LIMONITE_MODEL_DUMP=run/step7c/models cargo test -p limonite-client --bin limonite-client dump_every_model -- --ignored
  ```

  ```bash
  node scripts/compare-models-with-blockbench.mjs run/step7c/models
  ```

**The contact sheets** (m5-plan Step 7b): block states drawn through the real mesher, texture array
and shader, with no window, into labelled PNGs in `run/sheets`. Each tile is one state on a stone
floor, from the south-east and the north-west. They show Mojang's textures, so they stay gitignored.

```bash
LIMONITE_BEDROCK="C:/XboxGames/Minecraft for Windows/Content" cargo test -p limonite-client --bin limonite-client contact_sheets -- --ignored --nocapture
```

- `LIMONITE_SHEETS=doors,flat` draws only these sheets, and an empty value none (`SHEETS` in
  `render/sheet.rs` lists them).
- With `LIMONITE_JAVA_JAR` set, they show the jar's models, and otherwise the hand-made ones. A filter
  can ask about the Java state: `java.facing=south`.
- `LIMONITE_TEXTURES=door_lower,candle` draws those terrain keys' textures large, with a grid, into
  `textures.png`, for reading off a model's texture rectangles.
**Run the client: the scripted session** (`--headless`, the M2 and M3 acceptance run; no GPU and
no Bedrock install needed):

```bash
RUST_LOG=info,limonite_client=debug cargo run -p limonite-client -- --headless --map run/m3/map.png
```

Add `--edits 4` to place and break blocks with the real packets, on a stone floor it builds for itself at y 200, and
`--gamemode survival` for the server's times. Add `--inventory` to open the inventory in survival and in creative and
make the capture's moves (m5-plan Step 12). A new world's spawn is random, so a check has to go the same in any.

- **What it does:**
  1. Starts the embedded server and joins as the host.
  2. Walks 8 blocks east, has the server teleport it 5 blocks up, then waits for the join's
     chunks.
  3. Logs one `PASS`/`FAIL` line each for spawn, movement, operator, teleport, chunks, chunks
     decoded, world matches server, and light matches server.
  4. Writes the map, if asked.

  The exit code is non-zero on any failure.
- **Debug logging:** `limonite_client=debug` logs every packet name received, and the world
  check's first-pass differences.
- **Flags for both:**
  - `--home <path>`: the home folder, default `run/home`, gitignored (below).
  - `--data-dir <path>`: default `run/client`, gitignored. The client changes into it, because
    Pumpkin writes its world there.
  - `--name <name>`, `--radius <chunks>` and `--gamemode <mode>` override the settings file for
    one run, without saving. The name is 1 to 16 characters; the radius is 2 to 32, which the
    embedded server allows; the gamemode is survival, creative, adventure or spectator.
  - `--hotbar <items>` fills the hotbar on joining, through the server, as `LIMONITE_SHOTS_HOTBAR` does: up to 9 item
    names separated by commas, each with `*` and a count if it isn't 1. It's how to get blocks until Step 12's inventory
    screen. Headless, it adds a check that they arrived.
- **The home folder** (m5-plan Decision 16), written on the first run:
  - `settings.toml`: the name, the view radius, the field of view, the mouse sensitivity, the UI
    scale, smooth lighting, the gamemode for new worlds (creative), and the keymap, each with a
    comment. A missing or bad setting gets its default, and the file is rewritten; a good file is
    left alone. A file that isn't TOML is kept as `settings.toml.broken`.
  - `identity.toml`: this install's random 16-digit `xid`. The server derives the host's UUID from
    it, so worlds remember the player by it. A damaged file stops the client instead of quietly
    becoming a new player.
  - **The gamemode** applies to a host the world hasn't saved. A saved host keeps its own mode,
    with a warning.
- **The host is an operator:** before the server starts, the world's `data/ops.json` gets the host
  at level 4 if it isn't listed (Decision 18). A listed host keeps its level.
- **`--headless` only:**
  - `--linger <secs>`: stay in-world afterwards, for outside inspection; it logs the process id.
  - `--map <path.png>`: a top-down map of the loaded chunks, written after the checks.
  - `--stream <blocks>`: after the join, fly that many blocks east and log how chunks stream.
  - `--measure`: the cost table, chunk cost in phases, and the block-change probe. Only
    meaningful in a release build:

    ```bash
    CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo run --release -p limonite-client -- --headless --measure
    ```

    With `--measure` or `--stream`, the host rises to eye height 330 first (Gotchas, "The host
    has no physics").
- **Repeatable measurements:** run each on a fresh copy of one template world (`--data-dir
  run/m<N>-<label>`), as M3 and M4 did.
- **Run time:** a headless dev run takes about 25 s plus the linger. The world check's full pass is
  about 6 s in a dev build and under 1 s in release at radius 8. On a fresh world, `StartGame`
  came 1.85 s after the resource packs.
- **First builds:** about 3 minutes for dev, and 5 for release with the env vars. They compile
  `pumpkin-core` into the root `target/`, separate from `pumpkin/target/`.
- **Socket check** while it lingers, in PowerShell:

  ```powershell
  Get-NetTCPConnection -OwningProcess <pid>; Get-NetUDPEndpoint -OwningProcess <pid>
  ```

  Both should return nothing.

**Mojang server jar** (needed only when `pumpkin/assets/datapack/` is missing, e.g. a fresh clone)
- File: `run/mojang/server.jar`, Minecraft **Java 26.3**, from Mojang's version manifest.
- SHA-1: `33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c`, 62,294,556 bytes.
- Point the build at it with
  `PUMPKIN_MINECRAFT_SERVER_JAR="C:\Users\LaDoodleInTheHat\Desktop\big projects\limonite-client\run\mojang\server.jar"`.
  It must be a Windows-style path, because the build script is a native Windows program.

**Build the fork's server** (run in `pumpkin/`):

```bash
CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release --locked -p pumpkin
```

- Takes about 5.5 minutes from clean.
- The env vars skip the slow fat-LTO link without editing fork files. Always use the same ones,
  or Cargo rebuilds everything.
- Add `--no-default-features` for a Bedrock-only server: stock minus the Java listener. That's
  how M1 tested it. The output path is the same, so check which build `pumpkin.exe` is.

**Lint the fork** (run in `pumpkin/`): Pumpkin denies clippy pedantic and nursery lints.

```bash
cargo clippy -p pumpkin-core --all-targets
```

- Run it again with `--features java-listener`. Then run
  `cargo test -p pumpkin-core --lib net::bedrock`.
- For the client codecs, run `cargo clippy -p pumpkin-protocol --all-targets`, plain and with
  `--features client`. Also run `cargo clippy -p pumpkin-core --all-targets --features
  pumpkin-protocol/client`, because that's the feature-unified build the client produces. Then
  run `cargo test -p pumpkin-protocol --features client`.
- The `SLogin` golden test is `#[ignore]`d, because its capture can't be committed. Run it
  locally:

  ```bash
  LIMONITE_SLOGIN_FIXTURE="C:\Users\LaDoodleInTheHat\Desktop\big projects\limonite-client\run\captures\slogin.bin" cargo test -p pumpkin-protocol --features client --lib reproduces_the_official_client_login -- --ignored
  ```

- The M5 command and chat golden tests are `#[ignore]`d for the same reason. They read the
  capture's folder:

  ```bash
  LIMONITE_M5_CAPTURES="C:\Users\LaDoodleInTheHat\Desktop\big projects\limonite-client\run\captures\m5-official" cargo test -p pumpkin-protocol --features client --lib client_codecs -- --ignored
  ```

**Run the server**
- Run from `run/pumpkin/` (gitignored); it writes its world and config there:

  ```bash
  RUST_LOG="info,pumpkin_core::net::bedrock=debug" ../../pumpkin/target/release/pumpkin.exe
  ```

- `run/pumpkin/pumpkin.toml` overrides `networking.bedrock.online_mode = false`,
  `networking.java.enabled = false` and `telemetry.enabled = false`. Pumpkin fills in all other
  defaults on first start.
- Expect `Bedrock NetherNet signaling is listening on 0.0.0.0:19132`, then
  `Accepted Bedrock NetherNet connection` and `… joined the game` when the client connects.
- **In a Terminal tab, leave out the debug level** (m4-plan Deviation 16). NetherNet's
  per-datagram debug lines flood the tab, and Ctrl-C stops reaching the server. Plain INFO shows
  joins and commands, and stops cleanly on Ctrl-C.

**Official-client comparison** (M3 and M4): copy into the comparison folder, such as
`run/m4-official/`, only after a clean exit of our client:
- the world folder;
- `run/pumpkin/pumpkin.toml`;
- **`data/ops.json` with the owner at level 4** (copy `run/m3-official/data/ops.json`). Without it
  the server refuses every in-game command, while still logging it as executed.

Then run the stock fork server there, join from Bedrock, and type `/gamemode creative`,
`/time set noon`, `/weather clear` and the `/tp` our client logged with P. Screenshots stay in
the gitignored folder.

**Join from the official client**
- Bedrock → Play → Servers → Add Server, address `127.0.0.1`, port `19132`.
- Loopback works with no exemption needed.

**The README and the wiki**
- **The README** (`README.md`) is the GitHub landing page, kept current by CLAUDE.md's rule. Its
  current-phase alert, milestone badge and roadmap table follow this page.
  - **The title** (`docs/readme/title.png`) is a render of `docs/readme/models/title.bbmodel`, a
    Blockbench project in the Minecraft Title Generator plugin's format, with the lemon
    (`lemon.bbmodel`) beside the CLIENT plate. Re-render it with the plugin's Render: resolution
    2048, antialiasing on.
  - **The icons** on the feature cards are SVGs in `docs/readme/icons/`, stroked in the plate's
    lime, `#818600`. No emojis, at the owner's request.
  - **The screenshots** in `docs/readme/screenshots/` are the owner's gameplay shots from
    2026-10-25, cropped below the title bar and saved as 1600 px JPEGs. The before-and-after pair
    is one pose in Limonite and in the official client, taken with the comparison recipe above.
    Retake both once M5 draws light.
    - **`hud.jpg`** is the owner's pick from the screenshot tool (2026-11-01), retaken once the hotbar had
      items, and kept at 1920 px with no colour subsampling, so that the HUD's pixels stay sharp.
  - **After pushing a changed image,** github.com can show the old one for about 5 minutes, while
    GitHub's raw-file cache expires.
- **The org's page** (`https://github.com/LimoniteMC`) is `profile/README.md` in the separate
  `LimoniteMC/.github` repo, cloned into `run/org-profile` (gitignored). It repeats the milestone
  badge, the roadmap and the headline numbers, so they change there when they change in the README.
  Its images are absolute links to `main`, so a new one has to reach `main` first.
- **The wiki** (`https://github.com/LimoniteMC/limonite-client/wiki`) mirrors `docs/PLAN.md`,
  this page, `docs/recon/` and `docs/milestones/`, one page each. It's never edited by hand
  (CLAUDE.md, Rules).
  - **After committing doc changes,** run `pwsh scripts/sync-wiki.ps1`. It regenerates every page
    into `run/wiki`, a gitignored clone of the wiki, cloned if missing. It stops if the docs have
    uncommitted changes, and never commits or pushes.
  - **Then** check `git -C run/wiki diff`, and commit and push the wiki after `main`, with the
    owner's OK.
- **The owner's original models** from before the lemon (`title.bbmodel`, `lemon.bbmodel`, and
  their `.obj`, `.mtl` and texture exports) are in `run/readme-models-before-lemon/`, on the
  owner's machine only.

**The GitHub milestones** (`https://github.com/LimoniteMC/limonite-client/milestones`)
- **Why on this repo:** GitHub's milestones belong to a repo, and an organisation has none of its own.
  The fork's work is tracked here too, because the superproject is where a milestone is accepted.
- **Their numbers:** M0 to M4 are 1 to 5, and closed. M5 is 6, M5b is 7, and M6 to M11 are 8 to 13.
- **Their progress bars stay at 0%.** GitHub counts issues, and the steps are a checklist in the
  description. So read the checklist, and the milestone's state, rather than the bar.
- **After a push** (CLAUDE.md, Rules), change the description. For M5, once a step's commits are on `main`:

  ```bash
  gh api repos/LimoniteMC/limonite-client/milestones/6 --jq .description > run/milestone.md
  ```

  Tick the step in `run/milestone.md` (gitignored), then:

  ```bash
  gh api --method PATCH repos/LimoniteMC/limonite-client/milestones/6 -f description="$(cat run/milestone.md)"
  ```

- **When a milestone is accepted:** write "Done <date>." and what was accepted in its description, then
  close it, and write "Current." in the next one's. GitHub stamps the closing date itself, and can't
  backdate it, so the description holds the real one:

  ```bash
  gh api --method PATCH repos/LimoniteMC/limonite-client/milestones/6 -f state=closed
  ```

- **The page's order is backwards:** with no due dates, it has M11 at the top and the current milestone
  at the bottom. Read the titles. None are set, because the project hasn't any dates to give them.

## Gotchas

- **Long paths:** set `git config core.longpaths true` in any Pumpkin clone. Some paths exceed
  Windows `MAX_PATH`, and the checkout fails half-way without it.
- **NBT order:** `NbtCompound` keeps its tags in a `HashMap`, so two equal compounds serialise
  their tags in different orders. Compare decoded NBT in tests, never bytes, unless the
  compound has one tag.
- **Which structs go clientbound:** Pumpkin sends some serverbound structs to clients too:
  `SText`, `SActorEvent`, `SAnimate`, `SRespawn` and `SContainerClose`. Look in
  `bedrock/server/` as well as `bedrock/client/` when a packet id seems to be missing.
- **The host's position persists:** the dev world saves the player per UUID, which comes from
  the `xid` in `run/home/identity.toml`. Every run starts where the last one ended, so delete
  `run/client/` for a fresh world.
  - **M4's dev host** logged in with the fixed `xid` `1000000000000001`, so a world from before
    Step 4 sees the new identity as a new player. Put `xid = "1000000000000001"` in the identity
    file to be M4's host again.
- **Captures are private:** `run/captures/slogin.bin` contains the owner's Xbox token and
  identity. Never commit it, paste it, or print its contents.
- **Exact protocol:** Pumpkin accepts one Bedrock protocol only. Don't update the Bedrock client
  mid-milestone; if it updates past 2225, the official client can no longer join.
  - **The Store updates it unasked:** it took it to 1.26.60.5 on 2026-10-28, and the fork followed on 2026-11-04
    (`docs/upstream.md`). Check its version at each sync: `(Get-AppxPackage Microsoft.MinecraftUWP).Version`.
- **Stopping the server:** stopping a background shell doesn't always kill `pumpkin.exe` on
  Windows. Check with `Get-Process pumpkin`.
- **A commit message can close an issue, in any repo.** GitHub reads close, fix or resolve, in any form, followed by
  an issue reference, as an order, once the commit reaches a default branch and the pusher may close that issue.
  `ad2eb64` said Pumpkin ignores a pick "until upstream fixes Pumpkin-MC/Pumpkin#4321", and on the push of 2026-11-01
  that closed the owner's issue, as completed, with the bug still there.
  - **Write it another way:** "Pumpkin ignores it for now (Pumpkin-MC/Pumpkin#4321)". Only messages count, so a doc
    can say what it likes. Upstream's own commits are safe to carry, since the fork has no issues.
  - **Check before a push,** with the line breaks taken out, since a message can wrap between the two:

    ```bash
    git log origin/main..main --format=%B | tr '\n' ' ' | grep -ioE '\b(close[sd]?|fix(e[sd])?|resolve[sd]?):? +[^ ]*#[0-9]+'
    ```

- **Telemetry:** Pumpkin's defaults to **on** (heartbeat to `market.pumpkinmc.org`). Keep it off in
  every config.
- **Lockfile and profiles:** building from the repo root ignores Pumpkin's `Cargo.lock`,
  profiles and `rust-toolchain.toml`. The root has mirrored copies, so when the fork changes them:
  - **After an upstream sync,** copy `pumpkin/Cargo.lock` over the root `Cargo.lock` again and
    build. Shared crates then match the versions the fork was tested with. Cargo re-adds the
    client-only crates at their newest versions, so pin back any that moved (`docs/upstream.md`,
    the procedure's step 4).
  - **Profiles:** copy any new or changed ones into the root `Cargo.toml`. The `pumpkin-wasm-host*`
    overrides are left out on purpose: no root crate builds the plugin host.
- **Misleading banner:** a `--no-default-features` server with `networking.java.enabled = true`
  still prints `Connect using port: Java Edition: 0.0.0.0:25565`. That line is
  `crates/pumpkin/src/main.rs:194` echoing the config. Nothing is bound, so check sockets with
  `Get-NetTCPConnection -LocalPort 25565 -State Listen`, not the log. Left unfixed to keep the fork
  diff small; the embedded server never runs that `main.rs`.
- **Fork lints:** Pumpkin denies `clippy::use_self`, so code inside `impl … for NetherNetSession`
  writes `Self::send(..)`, not `NetherNetSession::send(..)`. It also denies `doc_markdown`, so put
  CamelCase words like `NetherNet` in backticks in doc comments.
- **Firewall:** Pumpkin binds `0.0.0.0`, so Windows Firewall may prompt. Allow it on Private
  networks only, or cancel; loopback works either way.
- **Untracked on purpose:** `.idea/` (RustRover) and `.claude/settings.local.json`.
- **The host has no physics.** The scripted walk keeps one height, so it can walk into a hillside.
  - **What happens:** the server suffocates the host: a hurt event every 0.5 s, dead in about
    10 s. Pumpkin then ignores the dead player's input, so it "stops moving".
  - **Why it carries over:** the dev world saves the host's position, so a later run can start
    inside terrain.
  - **Measured runs:** with `--measure` or `--stream`, the host rises to eye height 330 first.
  - **If acceptance runs start failing this way,** delete `run/client/` for a fresh world.
- **Pumpkin's log file keeps INFO only.** `logs/latest.log` in the server's directory has no
  debug lines, e.g. `Accepted Bedrock NetherNet connection`. Read those in the console.
- **Some block changes never reach clients.** Pumpkin's fire uses `NOTIFY_NEIGHBORS` alone
  (Upstream PR candidates). The world check tells that kind of drift apart from a decoder bug.
- **A running client locks its `.exe`.** On Windows, `cargo build` can't replace
  `target/debug/limonite-client.exe` while a window is open, so close it first. `cargo test` and
  `cargo clippy` neither touch nor rebuild it: run `cargo build` before starting the `.exe`
  directly, or it's stale.
- **Closing the window from a script:** `taskkill /PID <pid>`, without `/F`, sends `WM_CLOSE`. The
  host leaves, the world saves, and the exit code is 0. `/F` skips the save.
- **A harmless shutdown warning:** `Failed to send NetherNet packet … local client is gone` can
  appear as the client exits. The server sends a last packet after the host has left; nothing is
  lost.
- **The camera starts where the host was saved,** like the scripted run. Press P before closing if
  you'll want the spot again: the window can log a pose, but can't go to one yet.

Notes for M4 (found in M3) were all handled; see `docs/milestones/m4-plan.md`. Notes for M3 (found
in M2) were too; see `docs/milestones/m3-plan.md`.

## Notes for M5 (found in M2, M4's planning and M4)

M5's plan takes these up (`docs/milestones/m5-plan.md`): lighting and fog in Decisions 11 and 12,
special shapes in 7, the pose in 15 (a command line, so `/tp`), gamemode and collision in 5 and 6,
one server per process in 17, the identity in 16, the permission level in 18, block actions in
Step 2, and the ECS in 13. Plant offsets and tints stay out of scope, and M5 sends no new resource
pack status.

Found in M4:
- **Lighting:** chunks still carry no light data, and M4 draws only constant face shading. The
  sky, fog and clouds aren't drawn either.
  - **Fog will hide the view's edge.** Over water, rays that enter near the edge of the meshed
    area leave it before they reach the seabed, so a darker band of sky shows through the water
    (m4-plan Results, Step 10).
- **Plant offsets:** Java and Bedrock nudge flowers, grass and other plants off-centre per block.
  M4 draws every cross centred; only the bamboo stalk is nudged. Moving crosses, and bamboo's
  leaves, needs vertex positions that can leave their block: a bias in the vertex format. The outline
  stays where a plant is drawn (`shapes::outline_boxes`), so it has to move with them.
- **Tints:** one water colour everywhere (`#44AFF5` at 65%), no per-biome overrides and no blending
  between biomes. The official client's deep ocean is darker (m4-plan Decision 8).
- **Special shapes:** a to-do list, sorted by how often they occur, is under "Special shapes to
  do".
- **Going back to a pose:** the window can log one (P), but can't go to one. A `--pose` flag, or a
  teleport once the host has commands, would make comparisons easier.

Found earlier:

- **Gamemode and collision** (owner, when approving M4's plan): the host's gamemode is a setting
  (`--gamemode`), creative by default in M4.
  - **Why creative:** M4's fly camera passes through terrain, and a survival host with its head in
    a block suffocates.
  - **M5 revisits the default together with collision,** once the client has physics.
  - **Saved worlds:** the setting only applies to players without saved data. A saved host keeps
    its own mode (m4-plan Finding 11).
- **One server per process:** `SHOULD_STOP` and `STOP_INTERRUPT` are process globals, never
  reset. Switching worlds from a world list needs a process restart, or an upstream reset.
- **Host identity:** replace `login::DEV_XID` with a per-install identity saved with the
  settings. The player's UUID comes from the `xid`, so dev-world player data (inventory,
  position) won't carry over when it changes.
- **Permission level:** Pumpkin's Bedrock `StartGame` hard-codes 2 with a TODO. It's still an
  open question in PLAN.md.
- **Block actions:** the `PlayerAuthInput` writer refuses item, stack-request and block actions.
  Add those writers with block break and place.
- **Resource pack names:** only the HAVE_ALL_PACKS and COMPLETED status names are known (from
  the capture). Capture the others before sending them.
- **ECS:** pick `bevy_ecs` or `hecs` (PLAN.md open question).

## Notes for M6 (found in M4)

- **CPU meshing isn't worth optimising further** (owner, 2026-10-24): M6 replaces it with compute
  meshing. Parallel or worker-thread meshing comes back only if M6 keeps a CPU fallback path.
  - **Where it stands:** one sub-chunk at a time on the main thread, nearest first, within 4 ms a
    frame (m4-plan Decision 6).
  - **Its speed at radius 8:** 144 µs per sub-chunk in release, and 585 µs in a dev build. The
    client crate builds at `opt-level = 1` in dev builds for this; unoptimised, it took 1,956 µs
    (m4-plan Deviation 13).
- **The baseline M6 must beat** is in m4-plan Results, Step 11: frame times, draw calls and GPU
  totals at radius 8, 16 and 32. At radius 32 that's 166 fps, p99 15.6 ms, about 10,700 draw
  calls a frame, and 1.16 GB of vertex buffers.
  - **To compare,** run the same flight the same way: a release build with the env vars under
    Dev environment, and `--data-dir <a fresh copy of run/m4-template> --radius R --bench 30`.
  - **Keep `run/m4-template`.** It's gitignored, so it exists only on the owner's machine, and a
    new template gets a new seed, whose numbers won't compare.
  - **The flight leaves the template's area** at radius 16 and 32, so the server generates chunks
    during those runs. That's the same for M6, so it's fair, but a bigger template would take
    worldgen out of the numbers.

## Notes for M7 (found while planning M5)

- **Pumpkin trusts what a Bedrock client reports:** its position, unchecked, and the item it says
  it holds when placing (m5-plan Findings 1 and 3). PLAN.md's Risks has the row; M7 decides how to
  treat LAN guests.
- **StartGame tells every player permission level 2,** an operator (`world/mod.rs:2754-2755`, a
  TODO). M5 makes the host a real operator through `ops.json`, and reads the abilities packet
  instead, so a guest would be told it's an operator too. An upstream candidate.
- **Spawn protection is off** in the embedded config from M5 (m5-plan Decision 18), so guests could
  build at spawn.
- **Pumpkin ignores item interactions and item stack requests inside `PlayerAuthInput`**
  (`player_auth_input.rs:223-227`). Whether an official-client guest sends them there is for M7 to
  check.

## Special shapes to do (found in M4)

M4 drew every block with no shape of its own as a cube, and the owner asked for a to-do list of them.
M5's Step 7 drew them as Pumpkin's outline boxes, and Step 7b, after the owner's walk, gave most of
them models of their own (`blocks/models.rs`) and the right textures (`blocks/rules.rs`). The M4
census is in m4-plan's Results.

**What's left, with a jar** (m5-plan Step 7c, which compiles every other block's model from it):

| Blocks | What they are now | What they want |
| --- | --- | --- |
| Chests, ender chests, shulker boxes, banners, heads, decorated pots, conduits and copper golem statues | Boxes, turned the right way. The jar has no model for them | Entity models and entity textures, and their block actors. M8 |
| Beds | White, all of them | Their colour, in the block actor. M8 |
| What's in a flower pot | An empty pot | Its block actor. M8 |
| Redstone wire | A dot, coloured by its power | Its connections, which Bedrock works out from the blocks around |
| Chorus plants, fire and repeaters | A stem, fire on the floor, and never locked | The same |
| Lily pads and the like | One turn of each | Java's turn by position. Bedrock's way isn't known |

**Without a jar,** Step 7b's hand-made models draw (`blocks/models.rs`), and these are also left:

| Blocks | What they are now | What they want |
| --- | --- | --- |
| Beds, signs and hanging signs | Boxes. Beds show plain planks | The jar, or M8 |
| Tripwire and its hook | Plain strips, and a simple plate and hook | Java's small tilted parts |
| Scaffolding, dragon eggs, shelves and heavy cores | Pumpkin's boxes | Models of their own |
| Sculk sensors | A box without its tendrils | Four tendril planes |
| Open eyeblossoms, firefly bushes | Without their glow | A second, unlit layer |
| Pale moss carpet | A carpet | Its sides, which climb walls |
| Pitcher crops | Stages not drawn right | A model. Only grown from a sniffer's seed |
| Jigsaw blocks | Not turned | Their orientation. Only placed by a command |

**Not yet compared with the official client,** with or without a jar: the jar is Java's, so where
Bedrock's engine draws a block differently, this will too. Lit redstone torches are the first to check,
as they keep Java's glow boxes. The hand-made models were written from Java's, and are likeliest to
differ in leaf litter's and petals' quarters, the tilt of wall torches and levers, lectern tops, dripleaf
leaves as they tilt, and which way trapdoor and glazed terracotta textures turn. Nothing read from a jar
goes back into them (CLAUDE.md, Rules).

**How to look at them:** the contact sheets (Dev environment).

## Upstream PR candidates

**Upstream's rules for these** (its root `AGENTS.md`, read on 2026-10-26):
- a person opens every PR: maintainers close any PR an agent opened;
- the description carries a disclosure line, under its Description heading, saying an agent drafted
  it;
- a gameplay change needs a play-test, with a screenshot or recording attached;
- commit titles are conventional commits with a scope, such as `fix(protocol): ...`.

It also asks agents not to post on GitHub themselves. Our own rule already has the owner approve
every upstream PR. The 2026-10-26 issue (Pumpkin-MC/Pumpkin#4321) was opened from the owner's
account at the owner's request.

- **`Weather::downfall()`** (fork `443b954`, M4): a one-line accessor in
  `pumpkin-util/src/biome.rs`, in place of an `#[expect(dead_code)]` on the field. The client
  tints grass and foliage from each biome's temperature and downfall.
- **A base-dir parameter for Pumpkin**, so the embedding client needn't change the process's
  working directory (`main.rs`'s `enter_data_dir`). Pumpkin resolves the world, `ops.json`,
  bans, whitelist and user cache from the working directory.
- **The `client` codec feature in `pumpkin-protocol`**, with its golden fixtures.
- **Compression ignores its threshold:** every packet is deflated, even tiny ones (recon §5).
- **Faster Bedrock palette conversion:** fork commit `e251ffb`, which touches only
  `pumpkin-world/src/chunk/palette.rs` and cherry-picks onto upstream `master` unchanged. It
  replaces a SipHash lookup per block with a per-palette table. The output is byte-identical,
  checked against the old code kept as a test oracle. Before and after numbers are in
  `docs/milestones/m3-plan.md`, Results.
- **Read exactly one NBT compound** (fork `f9badd8`, the 2026-10-26 sync). Upstream's
  `PacketRead for NbtCompound` reads the rest of the packet before parsing, so it only works while
  the NBT is a packet's last field. The fork's reads exactly the compound
  (`pumpkin-protocol/src/serial/nbt_source.rs`), with tests.
- **Fire never tells clients about its changes** (found in M3, still in upstream `master`
  `5e9101f`). `block/blocks/fire/fire.rs` sets blocks with `BlockFlags::NOTIFY_NEIGHBORS` only in
  three places: fire that can't survive, fire put out by rain, and fire spreading. But
  `World::on_block_state_set` only queues a client update with `NOTIFY_LISTENERS`. So clients keep
  seeing fire that went out, and never see fire that spread. Vanilla uses flag 3 (`NOTIFY_ALL`)
  there. pumpkin-core has seven `set_block_state` calls with `NOTIFY_NEIGHBORS` alone, worth
  auditing together.
- **A second encode step, not tried yet** (m3-plan Deviation 12). The palette conversion is still
  about 89% of the faster encode: the packer divides by a runtime word size for each position,
  and `network_indices` builds a buffer per section. Measure it before proposing anything.

**Pumpkin bugs reported upstream** (found by the M5 capture, 2026-10-26; the owner chose to report
them rather than fix them in the fork). Each hits every Bedrock player, so our host too, until
upstream fixes it and a weekly sync brings the fix in:

- **No fall damage for Bedrock players:** Pumpkin-MC/Pumpkin#3396, already open. The Bedrock input
  handler (`net/bedrock/play/player_auth_input.rs`) never calls `LivingEntity::fall`, which the
  Java handlers do (`net/java/play/player_position.rs:157`, `:316`).
- **Block pick is always ignored:** Pumpkin-MC/Pumpkin#4321, opened by the owner's account on
  2026-10-26. `block_pick_request.rs:23-26` takes `max_slots` as the slot to fill, and the official
  client always sends 9, so the handler returns.
  - **Closed by mistake, and reopened:** a line in our own commit `ad2eb64` closed it on 2026-11-01 (Gotchas), and the
    owner reopened it on 2026-11-02. The handler hasn't changed, here or on upstream's `master`.
- **A survival placement costs two items:** Pumpkin-MC/Pumpkin#4130, already open. The official
  client's use-item transaction carries its predicted slot change, which `inventory_action.rs`
  applies (`:87-135`) before its own `stack.decrement(1)` (`:289`).

**Found since, not reported** (ask the owner before reporting any):

- **Experience from a save with no level:** `Player::read_nbt` calls `progress_in_level(level, points)`, which takes
  the points first (`pumpkin-util/src/math/experience.rs:86`). Only a save with `XpTotal` and no `XpLevel` goes that
  way (found 2026-11-03).

Notes for M2 (found while planning M1) were all handled; see `docs/milestones/m2-plan.md`.
