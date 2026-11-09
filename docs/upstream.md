# Upstream syncs

Limonite's Pumpkin fork (`LimoniteMC/Pumpkin`, branch `limonite`) follows upstream
`Pumpkin-MC/Pumpkin`, branch `master`. This page holds the procedure, and a log of every sync,
newest first: what came in, what it changed for Limonite, and how the conflicts were resolved.
`docs/STATUS.md` ("Where we are") says when the next sync is due.

## How the fork follows upstream

- **Weekly** (owner, 2026-10-26): once a calendar week, merge upstream `master` into `limonite`.
  A sync is due when the newest entry below is a week old. A session that finds one due does it
  before its own work, or tells the owner why it can't.
- **A merge, not a rebase** (owner, 2026-10-26).
  - **Why:** the fork is public, and every superproject commit pins a fork commit by its hash. A
    merge keeps all of them reachable. A rebase would rewrite `limonite`, need a force-push, and
    leave older superproject commits pointing at commits no branch holds.
  - **The fork's diff stays measurable:** `git -C pumpkin diff upstream/master` shows exactly
    what Limonite adds, whatever the history looks like.
- **A protocol bump waits for the owner.** Pumpkin accepts one exact Bedrock protocol, and the
  official client is the project's control (PLAN.md, Risks). A sync that would change the protocol
  stops before merging. The owner decides when to update the test client, ideally between
  milestones.
  - **The commits before the bump can still come in,** if the owner says so (2026-11-02): merge the
    bump's parent, not `master`.
  - **The Store doesn't wait:** it updates the official client by itself. Check it in step 1 with
    `(Get-AppxPackage Microsoft.MinecraftUWP).Version`. Past the fork's protocol, it can't join until
    the bump lands.

### The procedure

1. **Look first, change nothing.**
   - `git -C pumpkin fetch upstream`, then `git -C pumpkin log --oneline limonite..upstream/master`.
   - **The protocol:** `git -C pumpkin diff limonite upstream/master --
     crates/pumpkin-util/src/version.rs`. If the Bedrock protocol changed, stop and ask.
   - **Block data:** check whether `assets/blocks.json`, `assets/bedrock/` and
     `crates/pumpkin-data/src/generated/block*.rs` changed. If they did, saved worlds and
     `run/m4-template` may load differently, so tell the owner before going on.
   - **Conflicts:** `git -C pumpkin merge-tree --write-tree --name-only upstream/master limonite`
     lists them without touching anything.
2. **Merge:** `git -C pumpkin merge --no-ff upstream/master`, on a clean working tree.
   - Resolve conflicts by keeping both sides. A seam moves with the code it sits in; it's never
     dropped.
   - Also look for clashes that aren't textual, such as upstream adding an impl the client codecs
     also have. The fork checks below find them.
   - The merge commit's message lists each conflict and how it was resolved. A fix the merge needs
     goes in its own commit right after it.
3. **The fork checks** (STATUS.md, Dev environment, "Lint the fork"): clippy on `pumpkin-core`
   (plain, `--features java-listener`, and `--features pumpkin-protocol/client`) and on
   `pumpkin-protocol` (plain and `--features client`); then the tests, `pumpkin-core --lib
   net::bedrock`, `pumpkin-protocol --features client` and `pumpkin-world --lib chunk::palette`.
4. **The root lockfile:** copy `pumpkin/Cargo.lock` over the root `Cargo.lock`, then build. Cargo
   adds back the client-only crates at their newest versions, so compare them with the lockfile from
   before, and pin back any that moved with `cargo update -p <crate> --precise <version>`. Then the
   shared crates match what the fork was tested with, and the client's stay as they were.
   - **Profiles and specs:** if upstream changed a profile, or the spec of a dependency the root
     also names (such as `tokio`), mirror it in the root `Cargo.toml` (STATUS.md, Gotchas). The
     root's comment promises the same versions as `pumpkin/Cargo.toml`.
5. **The client:** `cargo build`, `cargo test --workspace`, `cargo clippy --workspace
   --all-targets`, and the headless checks (`--headless --map`). Adapt the client where upstream's
   API changed.
6. **The stock server:** rebuild it with STATUS.md's release command, so the official-client
   comparisons run on the merged code.
   - If upstream changed `net/bedrock/nethernet/` or the Bedrock login, ask the owner for a join
     from the official client.
7. **Log it here,** as a new entry at the top of the log:
   - the range, and the commits on both repos;
   - what changes for Limonite;
   - the conflicts;
   - the checks;
   - every upstream commit, by area.

   Then update "Where we are" in STATUS.md with the sync's date and the next due date.
8. **Commit:** the fork's merge (and any fix) on `limonite`; then on `main`, the bump with the
   lockfile and any client changes; then the docs. Ask the owner before pushing, fork first.
   - **A sync that changes `.github/workflows/` needs the `workflow` scope** on `gh`'s token, or
     GitHub refuses the push (found on 2026-11-01). Upstream's dependabot bumps those files often.
     Only the owner can add the scope: `gh auth refresh -h github.com -s workflow`.

## Log

### 2026-11-10: `a83abee` to `ac235ed`, 40 commits

- **Commits:** fork `d9cc703` (the merge); superproject `4cbd50e` (the bump, with the lockfile and the update checker
  off), `f1a3ca3` (the teleport check) and `33a1b75` (the latency probes). Upstream's commits are dated 2026-11-04 to
  2026-11-09. A day early, with the owner's go.
- **Bedrock protocol:** unchanged, 2225 (1.26.60). Pumpkin's own version is `0.2.2+26.3-26.60` now.
- **Block data:** unchanged. `assets/blocks.json`, the Bedrock palette and the generated block tables are identical,
  so saved worlds and `run/m4-template` load the same. World generation places the same blocks; only the villagers a
  structure spawns now take the biome they stand in (`ccdbb49`).
- **The official client** is 1.26.63.1 now (`1.26.6301.0`): the Store updated it after the join of 2026-11-05. A hotfix
  usually keeps its protocol, and upstream hasn't moved, but no join has checked it yet. The install gained a
  `vanilla_1.26.63` pack, which the client leaves out, being past the fork's 1.26.60.
- **No conflicts.** The fork's own diff is the same, file for file: `git diff a83abee 13bda25` and `git diff ac235ed
  d9cc703` both give 109 files, +4367 and -141, apart from line numbers.

**What it changes for Limonite**

1. **Bedrock movement goes through Java's path now** (`ddd13a2`, Pumpkin-MC/Pumpkin#3793). `apply_client_move` takes
   both editions' moves, which changes three things the host meets:
   - **The server ignores the host's input until it reaches a teleport,** within 0.1 blocks of the position sent, and
     sends the teleport again after every 20 inputs that don't. The client reports a teleport on its next input, as
     before. The "teleport" check now also waits for the server to stop waiting (`f1a3ca3`), so a run where the server
     ignores the host fails there.
   - **Fall damage reaches Bedrock players,** since the shared path checks it. In a survival run, the teleport check's
     5-block drop brings a hurt event, and then the health back every half second, as regeneration gives it.
     Pumpkin-MC/Pumpkin#3396 is still open, as it lists more than this.
   - **Standing on the ground** is read from the collision flag and the last input's `delta.y`, the motion the tick
     started with, as Geyser does.
2. **Entity pushes reach a Bedrock player as `SetActorMotion`,** tagged with its last input's tick. The official client
   rewinds to that tick and replays its inputs since. Ours sets the velocity when the packet arrives, which over the
   embedded server's round trip of about a tick comes to nearly the same. That's for Step 15, when mobs show.
3. **The server times the round trip with `NetworkStackLatency` probes,** once a second, and matches each answer by
   its timestamp. Its pushes wait a round trip after a motion of the player's own, and take an unmeasured one as 9
   ticks (`client_push.rs`). So the client answers each probe with its timestamp (`33a1b75`), and the headless run has
   a "latency" check: the server measured 19 to 40 ms. The official client's answer is yet to be held up against a
   capture.
4. **An update checker** (`4f77297`), on by default, asks `api.github.com` for Pumpkin's latest release at startup, and
   tells operators in chat when there's a newer one. The embedded config turns it off beside telemetry, and a new test
   holds the config to its promise that nothing opens a socket or reaches the internet. The stock server's
   `pumpkin.toml` turns it off too.
5. **Entities, for Step 15:** faster entity tracking (`3644ab7`), and villagers on a brain, with points of interest, a
   schedule, gossip and breeding, and parity work on 10 more mobs. The tracker still sends a Bedrock entity's moves
   every tick, as `MoveActorDelta` with only the fields that changed.
6. **Dependencies:** none moved. The root's lockfile is the fork's again, with `glam`, `ordered-float` and `toml_edit`
   pinned back to the client's versions. Only Pumpkin's own 13 crates changed, to 0.2.2.
7. **The other two Pumpkin bugs the M5 capture found** are still there: two items per placement
   (Pumpkin-MC/Pumpkin#4130, the `decrement(1)` after the predicted slot change, now at `inventory_action.rs:273`) and
   block pick (Pumpkin-MC/Pumpkin#4321, whose handler hasn't changed).

**Conflicts, and how they were resolved**

None.

**Checks**

- **Fork:** clippy is clean in all five configurations. The tests pass: 47 in `net::bedrock`, 227 in `pumpkin-protocol
  --features client`, and 13 palette tests. The 3 that read private captures pass when given them.
- **Client:** 403 client and 28 `packs` tests pass, the install's 2 as well, and clippy shows only its 2 older warnings.
  - `--headless --edits 4` passes in creative and in survival, 10 PASS lines each, in new worlds, and `--headless
    --inventory` with `--hotbar` gives 11. No run logs a warning.
- **The stock server** was rebuilt from the merge (5 min 36 s in release).
- **Not done:** an official-client join, which would show whether 1.26.63 still speaks 2225. Neither NetherNet nor the
  login changed, so the procedure doesn't ask for one.

**Every upstream commit, by area**

- **Players and movement:**
  - `ddd13a2` fix(entity): vanilla player touch area and push velocity sync (#3793)
- **Server and tools:**
  - `4f77297` feat: add update checker
  - `eda6944` chore: bump version to 0.2.2+26.3-26.60
  - `3644ab7` perf(world): optimize entity tracking and player spatial queries
  - `ae36c3a` fix(egg): add --init-settings and stop cleanly on install
  - `691c884` fix(egg): switch to pelican-eggs debian images for newer glibc
- **Villagers and golems:**
  - `a16320a` feat(entity): add a typed POI manager, villager schedule and village sensors
  - `9d09755` feat(entity): add villager job site and POI walking behaviors
  - `fcfd840` refactor(entity): move villager restock, composter and bone meal helpers out of the goal code
  - `9846e2d` feat(entity): add villager work, rest, panic, trade and door behaviors
  - `4b6ae0f` feat(entity): add villager sleeping, gossip, golem summoning and breeding
  - `36c768f` feat(entity): run villagers on the brain instead of goals
  - `080cd5b` fix(entity): stop a completed trade from deadlocking on the merchant screen
  - `68e114b` feat(entity): finish vanilla parity for villager
  - `ccdbb49` fix(entity): finalize structure villagers with the biome they generate in
  - `12d4d81` feat(entity): vanilla parity for iron golem
  - `c53fd57` feat(entity): add iron golem village stroll and return goals
- **Other entities:**
  - `3522114` feat(entity): vanilla parity for bat (#4304)
  - `8ca756a` feat(entity): vanilla parity for rabbit
  - `c116bc6` feat(entity): port mooshroom shearing, stew feeding and lightning conversion
  - `556733f` feat(entity): vanilla parity for tadpole
  - `56eed15` feat(entity): vanilla parity for endermite (#4481)
  - `6361843` fix(entity): add missing endermite ambient sound (#4495)
  - `593aac6` feat(entity): implement vanilla parrot behavior (#4486)
  - `d1190dc` fix(entity): check entity and border collisions when teleporting tamed mobs to their owner
  - `b27db4a` feat(entity): port frog brain AI and behaviors
  - `4042a9b` fix(entity): attack as the frog so magma cubes drop froglights
  - `725c9c1` feat(entity): vanilla parity for enderman (#4490)
  - `ce6d166` feat(entity): port sulfur cube
  - `ba1b65e` fix(entity): carry split data and piercing checks for sulfur cubes
  - `df5daf6` fix(entity): apply the sulfur cube fluid jump threshold everywhere
  - `3ea7d28` feat(entity): vanilla parity for cat (#4493)
- **World, items and advancements:**
  - `e0f536d` fix(worldgen): locate structures with the height sampler that generation uses
  - `494c29a` fix(item): apply suspicious stew effects from recipes, trades and loot
  - `576cca2` fix(advancement): stop structure location criteria matching everywhere
- **CI:**
  - `bc494ec` build(ci): add x86-64-v3 release and nightly builds
  - `ef039bf` build(ci): use x86-64-v3 binary when supported in Docker and egg
  - `6026a81` build(ci): bump Android NDK to r30
  - `4314c07` build(ci): drop musl targets and lower glibc baseline
  - `ac235ed` build(deps): bump crate-ci/typos from 1.51.1 to 1.52.0 (#4526)

### 2026-11-04: `eba538f` to `a83abee`, 64 commits, and Bedrock 1.26.60

- **Commits:** fork `4caebaf` (the merge), `22ebc6f` (what the merge needed to build) and `8acb34b` (the swing's
  golden test); superproject `010e373` (the bump, with the lockfile and the client's changes). Upstream's commits are
  dated 2026-10-29 to 2026-11-04.
- **The owner's go:** 2026-11-03, after the official client had updated itself to 1.26.60.5 (the last entry).
- **Bedrock protocol:** 2193 to **2225**, and Bedrock 1.26.51 to **1.26.60** (`f2cff8e`, Pumpkin-MC/Pumpkin#4407).
- **Block data:** changed. The Bedrock palette comes from CloudburstMC's 26.60 dump, 3.9 MB to 4.3 MB, with GeyserMC's
  mappings for Java 26.3 and Bedrock 26.60. The generated block table gained each block's sound type (`98e44e1`, for
  the sounds of movement).
  - **Saved worlds load the same:** a copy of `run/m4-template`, mapped with the client of 2026-11-01 and with this
    one, gives the same map, all 92,416 pixels of it.

**What it changes for Limonite**

1. **The official client can join again.** The fork takes protocol 2225, which the official client here has spoken
   since the Store updated it (m5-plan, Deviation 47). So Step 12's capture can go ahead, and the README's badge says
   1.26.60.
2. **Packets the client handles:**
   - **`Animate` ends with the hand now.** The client sends 0, the main hand (`session.rs`).
   - **`LevelChunk` ends with `client_biome_update`,** which Pumpkin always sends false. The client's decoder reads it
     and goes on (`world/level_chunk.rs`). Without that, every chunk had a byte too many, and 12 tests said so.
   - **`StartGame` gains `editor_level_migration_version`,** and `AddActor` and `AddPlayer` the block a passenger rides.
     The client codecs read them through the derives, with a client-side read for `PassengerOfBlockData` (`22ebc6f`).
   - **The use-item-on-entity and release-item transactions gain the hand.** The client sends neither yet.
   - **The golden fixtures are 1.26.52's.** `Animate`'s test adds the hand to its two (`8acb34b`), and the others still
     match. A capture of 1.26.60, Step 12's, replaces them.
3. **The vanilla packs:** the base game version is 1.26.60 now, so the client applies `vanilla_1.26.60` too, which it
   left out before. The install tests pass with it.
4. **NetherNet:** Bedrock's ping and LAN discovery go over NetherNet, and RakNet is gone (`87531ab`). The embedded server
   keeps NetherNet off, so the host doesn't notice. For M7's LAN, it's the one path now.
5. **The fork's NBT reader** decodes strings with pumpkin-nbt's own CESU-8 decoder, which upstream wrote in place of the
   `cesu8` crate (`22ebc6f`).
6. **Containers and items:** cursor drags and item stack request fixes (`338317a`), and container properties, mob
   equipment, links and block actors synced to Bedrock clients (`f023edb`). For Step 12 and M5b. A chunk brings more
   block actors now: 25 where the same world had 15.
7. **Serverbound lengths are bounded** before the server allocates (`a83abee`). The client's packets are well inside
   them: every headless check passes.
8. **Dependencies:** the MSRV is 1.97, and the root's `rust-version` follows. The root's lockfile is the fork's again,
   with `glam`, `ordered-float` and `toml_edit` pinned back to the client's versions. `cesu8`, `async-compression`,
   `itertools` and 8 more left the lockfile.
9. **The Pumpkin bugs the M5 capture found** are still open, and their code hasn't changed: no fall damage
   (Pumpkin-MC/Pumpkin#3396), two items per placement (Pumpkin-MC/Pumpkin#4130) and block pick (Pumpkin-MC/Pumpkin#4321).

**Conflicts, and how they were resolved**

- **`pumpkin-core/src/lib.rs`:** the `net::bedrock` imports keep the fork's `local::LocalListener` and
  `transport::Transport`, and take `IceSocket` from `nethernet`, where upstream moved it when it removed the RakNet
  status module.
- **`pumpkin-core/src/net/bedrock/mod.rs`:** the fork's `transport` module stays, and `status` goes, as upstream removed
  it.
- **Not textual:** the `cesu8` crate (item 5), and the client codecs' new fields (item 2).

**Checks**

- **Fork:** clippy is clean in all five configurations. The tests pass: 45 in `net::bedrock`, 217 in `pumpkin-protocol
  --features client`, and 13 palette tests. The 3 that read private captures pass when given them.
- **Client:** 357 client and 28 `packs` tests pass, the install's 2 as well, and clippy shows only its 2 older warnings.
  - `--headless --edits 4` passes in creative and in survival, 8 PASS lines each, in new worlds, and `--headless --map`
    with `--hotbar` gives 8. The edits check's refused click logs its new line.
  - A survival run's log has the same one warning as before.
- **The stock server** was rebuilt from the merge (6 min 55 s in release).
- **The official client joined** the stock server from the merge, over NetherNet, on 2026-11-05: Bedrock 1.26.60.5,
  found under LAN games through NetherNet's discovery on port 7551. That's the join the procedure asks for, since
  NetherNet and the login changed.

**Every upstream commit, by area**

- **Bedrock:**
  - `f2cff8e` feat(bedrock): port protocol to 1.26.60 (#4407)
  - `87531ab` feat(bedrock): use nethernet for ping and remove raknet (#4427)
  - `338317a` fix(bedrock): handle cursor item drag packet and fix item stack request actions (#4428)
  - `2709e8b` feat(bedrock): added some 26.60 bedrock packets
  - `f023edb` feat(bedrock): sync container properties, entity equipment, links, and block actors
- **Protocol:**
  - `0f826e2` fix(protocol): use CSwingArm for offhand swings on 26.3+ (#4338)
  - `678891b` fix(protocol): encode armor trims as registry references (#4101)
  - `5db8510` fix(protocol): send the out of stock flag in merchant offers
  - `a83abee` fix(protocol): bound serverbound collection lengths before allocating (#3911)
- **Server and tools:**
  - `919cac1` fix(console): stop the piped console reader when the server stops (#4325)
  - `55cb804` chore: update MSRV to 1.97.0 (#4425)
  - `e17b85a` fix(gametest): Update GameTest structure version handling (#3919)
- **Plugins:**
  - `5152bb5` fix(plugin-wit): map protocol i8 fields to WIT s8 in v0.2 (#4413)
  - `6458950` fix(plugin-wit): revert Bedrock 26.60 packet changes in v0.1
- **Commands:**
  - `c6bb20e` fix(command): send the 26.3 argument parser ids and parse registry ids like vanilla (#4311)
  - `9427b25` fix(command): skip structures that cannot generate in the dimension in /locate (#4446)
  - `019f118` fix(command): resolve local coordinates from the source position like vanilla (#4324)
- **World:**
  - `83efbd3` perf(world): optimize noise router, structure placement, and chunk generation
  - `ca480ba` fix(world): check redstone conductors above in-water spawn positions
  - `62aac84` feat(world): spawn wandering traders with their trader llamas
  - `ade678e` fix(world): check spawner spawn rules and strike thunder lightning
  - `d9a7863` fix(fluid): bring lava closer to match vanilla (#4464)
- **Blocks and items:**
  - `98e44e1` feat: movement emission (#4408)
  - `2064797` feat(block): bone meal on moss and pale moss places a vegetation patch (#4442)
  - `3cc5932` fix(dispenser): match vanilla 26.3 projectile spawn position and direction (#4399)
  - `1a5e631` fix(item): match vanilla bucket placement replacement (#4289)
- **Players:**
  - `b6eca28` feat(player): port recovery compass LastDeathLocation (#4424)
- **Entities:**
  - `62fdda6` perf(entity): stop preallocating every mob's pathfinding heap (#4053)
  - `5a24650` fix(entity): play vanilla fall damage sounds (#4409)
  - `06bcb26` fix(mobai): Temperament enum, data-driven hunt/flee tables, vanilla target search (#4107)
  - `90ac8d8` fix(entity): add the water splash effect and fix pufferfish stings
  - `d67df7e` feat(entity): port vanilla dolphin goals, air supply and treasure search
  - `7596c80` fix(entity): let any living first passenger control a boat
  - `423e6c1` feat(entity): pick up loot through a shared mob ai step
  - `743b602` fix(entity): roll CanPickUpLoot on finalize spawn instead of every spawn
  - `829302f` feat(entity): store saddles in the saddle equipment slot (#4322)
  - `efaa388` fix(entity): remove the duplicate nautilus controlling passenger
  - `e91d51d` feat(entity): port llama and trader llama parity
  - `f68f819` fix(entity): let thrown projectiles hit players
  - `13b00d2` fix(entity): step mobs up onto blocks within their step height
  - `a834407` fix(entity): make leashed mobs walk after their leash holder
  - `d6238bc` fix(entity): roll animal experience on every death
  - `9c78298` fix(entity): stop ridden horses from being pushed
  - `37010ae` fix(entity): port mob fall damage overrides and passenger propagation
  - `1c4b56a` fix(entity): limit dispenser equipping to mobs that accept it
  - `3b2809a` fix(entity): check fall damage on player-driven vehicle moves
  - `04d4403` fix(entity): add the equipment bonus to mob experience
  - `16fa5de` fix(entity): port the bat, parrot, creaking and warden push rules
  - `197b620` fix(entity): port the elastic leash spring and angular momentum
  - `1fd8857` feat(entity): pass the spawn reason to finalize_spawn
  - `ff3be51` fix(entity): give slimes and baby zombies their vanilla experience
  - `0bf0401` fix(entity): port the horse followMommy for llamas
  - `c730387` style(entity): format the zombie experience reward
  - `76c09c9` fix(entity): make polar bears defend nearby cubs (#4448)
  - `3e812f8` refactor(entity): move the horse family onto AbstractHorse
  - `296860b` feat(entity): port the horse family
  - `45d7a89` feat(entity): seat riders on their vehicle
  - `ffa0fd2` feat(entity): port the camel and close the horse family gaps
  - `c178719` fix(entity): wear armor and helmets when hurt
  - `46a21c0` fix(entity): seat and size entities from extracted data
  - `06f24b7` fix(entity): stop turning a camel's body while it sits
  - `a3d7cd4` fix(entity): close the cow gaps and fix animal feeding
  - `8407592` fix(entity): close the chicken gaps
  - `2d6a9a4` fix(entity): close the wandering trader gaps

### 2026-11-02: `5e9101f` to `eba538f`, 25 commits

- **Commits:** fork `2c52f24` (the merge); superproject `1b226d9` (the bump). Upstream's commits are dated 2026-10-26 to
  2026-10-29.
- **It stops short of upstream's `master`, by the owner's choice.** Upstream's next commit, `f2cff8e`
  (Pumpkin-MC/Pumpkin#4407), ports to Bedrock 1.26.60. It and the 14 after it wait ("What waits", below).
- **Bedrock protocol:** unchanged, 2193.
- **Block data:** unchanged. `assets/blocks.json`, the Bedrock palette and the generated block tables are identical,
  so saved worlds and `run/m4-template` load the same.
- **Lockfiles:** unchanged. No manifest, profile or toolchain file moved, so the root's `Cargo.lock` and `Cargo.toml`
  stay as they were.

**What it changes for Limonite**

1. **Breaking and placing follow the same rules.** `BlockRegistry::placement_target` and
   `can_replace_with_other_block` didn't change, and nothing touched how long mining takes, so `edit::plan_use` and
   `edit::breaking_speed` still say what the server says. `--edits` agrees, in both gamemodes.
   - Snow now gains its layers through `can_update_at`, and snow, candles, vines and sculk veins lost their
     `use_with_item`. A block onto itself was already left to the server (`plan_use`).
   - Of the blocks that react to a click, flower pots gain `normal_use` (`reacts_to_use` has them by tag), and vaults
     lose theirs (they were never on the list).
2. **The server has new thread pools** (`b70c7e1`), and each `Server` owns them, so the embedded one does too.
   - **`Simulation-*`,** one thread per CPU, runs the tick. `install` runs a closure inside a pool, so every `par_iter`
     in the tick uses that pool and not Rayon's global one.
   - **`Compute-*`,** half as many (2 to 8), takes crypto, compression and Bedrock's chunk encoding.
   - **Rayon's global pool** is left with reading saved chunks and the client's item icons, so `set_up_process`'s
     comment changed. World generation has had its own pool, `ChunkGen-*`, since before the fork began.
   - An idle pool costs no CPU. Step 17's bench will show if the split matters to frame times.
3. **Bedrock's login checks more** (`8e21ebc`): the whitelist, bans, the player limit, a duplicate name or UUID and the
   plugin pre-login event, as Java's does. The host passes, as every headless run shows. **For M7:** a guest with the
   host's name is turned away. This touches the login, so the procedure's step 6 asks for an official-client join,
   which has to wait (item 7).
4. **More of what a Bedrock client sends now does something** (`9d9179d`, Pumpkin-MC/Pumpkin#4397).
   - **Movement:** the server acts on the gliding, swimming and spin attack flags in `PlayerAuthInput`, and on those
     and sneaking, sprinting and crawling in `PlayerAction`. Our client only sets sneak and sprint
     (`player/movement.rs`), so nothing changes yet.
   - **The hotbar:** `SPlayerHotbar` selects a slot now, as `MobEquipment` does. The client sends `MobEquipment`.
   - **Attacks check reach** (`can_interact_with_entity`), with a block's grace. For Step 15.
   - **Item stack requests** gain anvil, beacon, grindstone and loom actions, and answer `MineBlock` with the slot's
     stack. For Step 12 and M5b.
5. **Datapacks are read at runtime** (`cf4e476`, Pumpkin-MC/Pumpkin#4370): recipes, tags, predicates, item modifiers,
   slot sources and dynamic registries. That bears on PLAN.md's open question, how the embedded server gets the vanilla
   datapack without the binary bundling it. It isn't settled, and nobody has looked further yet.
6. **World generation:** `edfbb48` aligns features with vanilla, and changes the stage hashes in `assets/tests/`, so
   new chunks can differ from the same seed's chunks before. Chunks already saved don't change. `eba538f` is upstream's
   cut of about a quarter in generation's CPU.
7. **The official client can't join for now.** The Microsoft Store updated it to 1.26.60.5 on 2026-10-28, and the fork
   only takes 2193, so it waits for the bump (m5-plan, Deviation 47). Comparing looks still works, with the official
   client in a world of its own.
8. **The Pumpkin bugs the M5 capture found:**
   - **No fall damage:** Pumpkin-MC/Pumpkin#3396 is still open, and `player_auth_input.rs` still never calls
     `LivingEntity::fall`.
   - **Two items per placement:** Pumpkin-MC/Pumpkin#4130 is still open, and that code in `inventory_action.rs` didn't
     change.
   - **Block pick:** a line in our own commit `ad2eb64` closed Pumpkin-MC/Pumpkin#4321 by mistake on 2026-11-01
     (STATUS.md, Gotchas), and the owner reopened it. The handler hasn't changed, here or on upstream's `master`, so a
     pick is still ignored.

**What waits**

Upstream's 15 commits from `f2cff8e` to `83efbd3`. They bring the port to 1.26.60, NetherNet for ping with RakNet removed
(`87531ab`), and fixes to cursor drags and item stack requests (`338317a`), which Step 12 will want.

- **Why it's more than a merge:** a dry run against this merge conflicts in `pumpkin-core/src/lib.rs` and
  `net/bedrock/mod.rs`, and `assets/bedrock/` changes, so saved worlds and the client's tables need checking.
- **Step 1's capture is of 1.26.52,** so the client's packets need comparing with a new one on 1.26.60.

**Conflicts, and how they were resolved**

None. The fork's own diff is the same, file for file: `git diff 5e9101f 15745d6` and `git diff eba538f 2c52f24` both
give 98 files, +4147 and -141.

**Checks**

- **Fork:** clippy is clean in all five configurations. The tests pass: 45 in `net::bedrock`, 210 in `pumpkin-protocol
  --features client`, and 13 palette tests. The 3 ignored there, the golden tests that read private captures, also pass
  when given them.
- **Client:** 348 client and 28 `packs` tests pass, and clippy shows only its 2 older warnings.
  - `--headless --edits 4` passes in creative and in survival, 8 PASS lines each, in new worlds, and again on the final
    release build. `--headless --map` gives 7 PASS lines.
  - The embedded server stops through `Server::stop` and saves ("Completed save!").
  - A survival run's log has the same one warning as before the merge.
- **The stock server** was rebuilt from the merge (4 min 54 s in release).
- **Not done:** an official-client join over NetherNet on the merged server, and the creative-take capture (item 7).

**Every upstream commit, by area**

- **Server:**
  - `b70c7e1` refactor(core): decouple tick simulation, compute pools, and Tokio runtime
- **Bedrock:**
  - `8e21ebc` fix(bedrock): enforce whitelist and bans on login
  - `9d9179d` feat(bedrock): implement inventory transactions, combat parity, and player movement (#4397)
- **Players:**
  - `2cf1569` fix(player): save player data on the server runtime from the tick thread (#4335)
- **Entities:**
  - `53a0bb9` feat(entity): port squid and glow squid to vanilla behavior
  - `4f4f505` fix(entity): close shared mob parity gaps
  - `638bead` fix(entity): port vanilla entity save data (#4396)
  - `de9538a` fix(entity): attribute projectile block hits to the correct block (#4179)
- **Blocks, items and data:**
  - `0a9ddfd` fix(block): align block behaviours with vanilla
  - `4c8c7f3` fix(item): align item behaviours with vanilla
  - `d1c399c` fix(block): relight campfires hit by burning projectiles (#3555)
  - `48990c4` feat(block): dispense experience bottles as projectiles (#3645)
  - `027ef56` feat(advancement): implement criteria triggers with loot predicate evaluation (#4369)
  - `cf4e476` feat(data): implement DynamicRegistryManager with runtime datapack loaders (#4370)
- **World generation:**
  - `edfbb48` fix(world): align feature generation with vanilla
  - `eba538f` perf(worldgen): cut chunk generation CPU by about a quarter (#4401)
- **Protocol and codecs:**
  - `59eecff` fix(protocol): read zero LpVector3d from a single byte (#4308)
  - `5061cf8` refactor(codec): improve deserialization by adding size checks and a reusable read_count function (#4394)
- **Plugins:**
  - `1ec04f6` feat(plugin): expand v0.2 player and inventory APIs for Bukkit parity
- **Tests and CI:**
  - `463274e` test(bot): align allay interact polling timeout with standard bot poll ticks
  - `86f36e0` fix(ci): skip doctests for affected packages without a library (#4381)
  - `ff873cb` fix(docker): switch to glibc base image and standard binary (#4390)
  - `09e63cb` Add link to CI Badge (#4170)
  - `ee25c91` chore(deps): bump crate-ci/typos from 1.50.2 to 1.51.1 (#4319)
  - `ac98a5d` chore(deps): bump actions/setup-python from 6 to 7 (#4318)

### 2026-10-26: `aac0a82` to `5e9101f`, 44 commits

- **Commits:** fork `845450b` (the merge) and `f9badd8` (the NBT reader fix); superproject
  `a8f0271` (the bump). Upstream's commits are dated 2026-10-18 to 2026-10-26.
- **Bedrock protocol:** unchanged, 2193 (1.26.51, which Bedrock 1.26.52 also speaks).
- **Block data:** unchanged. `assets/blocks.json`, the Bedrock palette and the generated block
  tables are identical, so saved worlds and `run/m4-template` load the same.

**What it changes for Limonite**

1. **A server's lifecycle is now apart from the process** (`5e9101f`, Pumpkin-MC/Pumpkin#3771).
   - `PumpkinServer::new` takes `standalone: bool`, and returns a `ServerSetupError` instead of
     exiting the process.
   - A non-standalone server reads no console, never marks the process as stopping, and
     `Server::stop` ends only that server. Each `Server` has its own stop token, a child of the
     process-wide one.
   - **The client** now builds the embedded server as non-standalone and stops it with
     `Server::stop`. Its `LOGGER_IMPL` workaround is gone.
   - **For M5:** m5-plan Finding 13's reason for one server per process no longer holds.
     Decision 17's supervisor is to be looked at again before Step 16 (m5-plan, Deviations).
2. **NBT reading** (`098e104`, fixed by the fork's `f9badd8`).
   - Upstream added `PacketRead for NbtCompound` for its new `BlockActorData` packet. It read to
     the end of the packet, which swallows every field after a compound.
   - The fork's version reads exactly the compound, and the client codecs' own copy, which
     clashed with it, is gone. An upstream candidate (STATUS.md).
3. **New Bedrock packets the client can receive:**
   - `SetActorData`, with mob metadata (`f5ae2bc`);
   - `BlockActorData` and `OpenSign`, from block actor sync and sign editing (`098e104`);
   - resource pack streaming: `ResourcePackDataInfo` and `ResourcePackChunkData`, and the
     serverbound `ResourcePackChunkRequest` (`459ec12`). The embedded config has no resource
     packs, so the host's join is unchanged.
4. **Gameplay** that the host will see:
   - mob AI reworked (`a1867b3`), and vanilla behaviour for cows, chickens, sheep, cod, salmon,
     pufferfish and tropical fish; allays collect and deliver items;
   - mobs despawn at their category's distance;
   - crops need light level 9 to grow.
5. **World generation:** snow on the top layer now checks for a supporting face and the light
   (`593a45e`), so new chunks can differ from the same seed's chunks before. Chunks already saved
   don't change.
6. **Java 26.3 items and data components** (`fb7c8e2`): the generated item tables changed. M5
   Step 9's item mapping is built on them.
7. **Dependencies:** wasmtime 50 for plugins; tokio 1.54, uuid 1.29 and lru 0.19.
   - The root `Cargo.lock` is the fork's again, and `toml_edit` is pinned back to 0.25.15, the one
     client-only crate that moved.
   - M5 takes `uuid` at Pumpkin's 1.29 (m5-plan Decision 22 said 1.28).
   - The root `Cargo.toml` mirrored `tokio` 1.54 in a follow-up commit, which the procedure now
     covers.
8. **Still open upstream:** the Pumpkin bugs the M5 capture found. No fall damage for Bedrock
   players is Pumpkin-MC/Pumpkin#3396, and two items used per placement is Pumpkin-MC/Pumpkin#4130,
   both reported before. Block pick ignored is Pumpkin-MC/Pumpkin#4321, opened on 2026-10-26 (STATUS.md,
   Upstream PR candidates).

**Conflicts, and how they were resolved**

- **`pumpkin-core/src/lib.rs`:**
  - upstream moved the Java bind into a new `setup()`, as `bind_java`, which returns errors. The
    `java-listener` seam now gates that call, so without the feature there's still no Java
    socket, query or LAN broadcast;
  - `PumpkinServer` keeps `local_listener` beside upstream's new `standalone` field, in the
    struct and in its constructor;
  - `accept_bedrock` and the local accept branch merged cleanly, beside upstream's new branch for
    the server's stop token.
- **`pumpkin-util/src/biome.rs`:** `downfall()` sits beside upstream's new `has_precipitation()`.
- **`pumpkin-world/src/chunk/palette.rs`:** only the test module's `use` line, which takes both
  sides' names. Upstream's count fixes and `contains_matching` sit beside the fork's speedup, and
  its oracle tests still pass.
- **Not textual:** the two `PacketRead for NbtCompound` impls (item 2).

**Checks**

- **Fork:** clippy is clean in all five configurations. The tests pass: 43 in `net::bedrock`, 150
  in `pumpkin-protocol --features client` (1 ignored, the local `SLogin` test), and 13 palette
  tests.
- **Client:** 75 client and 26 `packs` tests pass, and clippy is clean.
  - `--headless --map` gives five PASS lines. The "changed by the server without an `UpdateBlock`"
    warnings are fire, as in M3.
  - The embedded server stops through `Server::stop` and saves ("Completed save!").
- **The stock server** was rebuilt from the merge (7 min 14 s in release).
- **Not yet done:** an official-client join over NetherNet on the merged server. The owner moved
  it to M5 Step 12, together with the creative-take capture.

**Every upstream commit, by area**

- **Server:**
  - `5e9101f` feat(server): decouple server lifecycle from the process for embedding (#3771)
  - `1e5f0ed` perf(startup): compute stronghold rings in the background and skip key fetch for
    unsigned plugins (#3764)
  - `09ec006` chore: bump version to 0.2.1+26.3-26.51
- **Bedrock:**
  - `459ec12` feat(bedrock): stream resource packs to connecting clients
  - `098e104` feat(bedrock): synchronize block actor data and handle sign editing
  - `f5ae2bc` feat(entity): abstract mob metadata for java and bedrock
- **Entities and AI:**
  - `a1867b3` refactor(entity): rework the mob AI foundation (#4092)
  - `3cf12e8` fix(entity): bring cows in line with vanilla (#4171)
  - `2a7b387` fix(entity): bring chickens in line with vanilla (#4175)
  - `1feb9d8` fix(entity): bring sheep in line with vanilla (#4173)
  - `56896b7` feat(entity): add allay item collection, delivery and dancing (#4174)
  - `0975db8` feat(entity): vanilla parity for cod (#4284)
  - `96424b8` feat(entity): vanilla parity for salmon (#4291)
  - `d4bedb4` feat(entity): vanilla parity for pufferfish (#4302)
  - `99d31d0` feat(entity): vanilla parity for tropical fish (#4303)
  - `fe900d0` fix(entity): despawn mobs at their category distances
  - `cd9e98c` fix(entity): send each species' collar color in its own data slot (#4286)
  - `e11beaf` fix(entity): handle release const lint in island reach check
  - `683e140` fix(spawner): compute spawn potential distance in f64 (#3891)
- **Players:**
  - `3444552` fix(player): correctly handle off-hand item pickup (#4104)
  - `44d881c` fix(player): center sleeping position, skip in-wall damage while asleep (#4097)
- **Blocks, items and data:**
  - `fb7c8e2` feat(core): implement 26.3 blocks and data components
  - `52dab9d` fix(block): require light level 9 for crop growth (#4100)
  - `f4dcfd4` fix(block): play copper chest sounds for each oxidation stage (#4071)
  - `8c4d18c` fix(block): allow placing blocks against redstone ore (#4025)
  - `a7f2424` fix(item): apply use_remainder when finishing item use (#3977)
  - `5cc62c5` fix(codegen): generate block transformer rules (#3982)
  - `9ae7627` fix(loot): preserve table structure, predicates and modifier order (#4186)
- **World generation:**
  - `593a45e` fix(worldgen): gate freeze-top-layer snow on face support and light (#3438)
  - `328692d` fix(worldgen): count each structure once in the beardifier (#4037)
  - `c7a234a` fix(world): send the flat world flag to Java clients (#4102)
- **Commands:**
  - `57fe06a` fix(command): run placed callbacks for blocks placed by /fill (#3964)
  - `3e076ea` refactor(command): move contributors and sponsors into pumpkin subcommand
  - `697d36a` fix(command): parse item components like vanilla
- **Java protocol:**
  - `8ca5cb4` fix(protocol): send variant components with vanilla network ids (#4285)
- **Plugins:**
  - `f313626` feat(plugin): expose APIs needed by PatchBukkit
  - `8ea2b7d` fix(plugin): correct GUI slots and inventory synchronization (#4157)
  - `9c04f71` feat(plugin): load v0.1 plugins on a sequential engine for upgrading to wasmtime
    50.0.0 (#4098)
- **Tests and CI:**
  - `e077274` feat(gametest): add more built-in game tests
  - `54ae17f` test(tester): increase headless bot packet timeout for slow CI environments
  - `3a6ea85` test(tester): increase headless bot poll timeout for slow CI environments
  - `14405c4` chore(deps): bump DeterminateSystems/magic-nix-cache-action (#3442)
  - `c289be1` chore(deps): bump actions/github-script from 8 to 9 (#3441)
  - `224f9f8` chore(deps): bump crate-ci/typos from 1.50.1 to 1.50.2 (#3662)

The numbers in brackets are upstream's pull requests.
