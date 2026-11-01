# Milestone 5 plan: Playable singleplayer

**Status: approved on 2026-10-24; under way. Step 1 was done on 2026-10-26.** The owner answered all fifteen questions as
recommended ("Questions and answers", at the end), and asked for five changes, all folded in below:
- the physics is checked against the official client: Step 1's captured `PlayerAuthInput`
  positions are the ground truth for a jump's peak, walking, sprinting and sneaking speeds, and
  falling, and our physics replays the same inputs (Decision 5, Steps 1 and 6);
- Step 7 has a test that walks every Bedrock id, checks that all the Java states sharing it have the
  same collision and outline boxes, and lists any that don't (Decision 7, Step 7);
- each owner checkpoint is a go/no-go point. If a phase overruns, the owner can tick what's done
  and split the rest into its own milestone (Decision 1, Owner involvement);
- PLAN.md's Risks now say that Pumpkin trusts Bedrock movement and placement, so M7 must decide how
  to treat LAN guests (Decision 4);
- the supervisor goes back to the world list with an error when a child crashes, and stops with a
  message if a child dies at startup twice in a row (Decision 17, Step 16).

Question 3's answer is in PLAN.md too: container screens are milestone 5b, straight after M5.

The plan below is kept as approved; "Deviations" at the end will record where the real code
differs.
- **Before starting:** read `docs/STATUS.md` ("Notes for M5", "Special shapes to do", Dev
  environment, Gotchas), the Deviations in `m1-plan.md` to `m4-plan.md`, and
  `docs/recon/m0-pumpkin-bedrock.md` before touching the fork.
- **If the real code contradicts this plan,** stop and tell the owner (CLAUDE.md rule).
- **As you go:** teach. M5 is the project's first game programming: a fixed-step simulation,
  collision, prediction, light propagation and a UI framework. Explain the why behind each step,
  especially the ones marked "worth teaching". Update `docs/STATUS.md` as steps land.

## Context

M4 put the world on screen, but the host is still a camera. It flies through terrain, can't touch a
block, and sees no light, mobs, HUD or menus. M5 makes it a game to play alone. The host walks with
gravity and collision, breaks and places blocks, sees mobs move smoothly, and sees the world lit by
the sun and by torches. It gets a HUD, an inventory and a world list, and its settings and identity
persist between runs.

Three things are new for the project:
- **The client simulates.** The local player moves by its own physics every tick, ahead of the
  server, and edits are drawn before the server confirms them. That's PLAN.md's client-side
  prediction.
- **The client works out what the server never sends:** light (PLAN.md, Lighting).
- **The client gets its own UI** (PLAN.md, UI): a retained widget tree drawn through wgpu, with
  sprites, the font and the strings read at runtime from the install.

The fork changes only by client codecs, as in every milestone so far: readers and writers for the
play packets M5 uses, golden-tested against a new capture from the official client.

**Accept when:**
- **PLAN.md M5:** a full session of walking, building and saving with no visible input lag; reload
  restores the world.
- **This plan adds:**
  - **"No visible input lag", measured** (Decisions 3 and 9): mouse look moves the camera in the
    next frame; a movement key moves the player from the next tick, as in both vanilla clients; a
    predicted break or place is drawn in the next frame. The log summarises each, and the frame
    times, at the end of a session;
  - **"Reload restores the world", checked:** after Save & Quit and reopening from the world list,
    the blocks, the inventory, the host's position and gamemode, and the time of day are as they
    were. A two-run headless check does the same for scripted edits;
  - **light:** the client's sky and block light equal the server's own light engine, or each
    difference is explained (a headless check, like M3's world check);
  - entities move smoothly between server updates;
  - the world list creates, opens, copies and deletes worlds;
  - the headless M2 to M4 checks still pass, and `--bench` still flies M4's flight;
  - the owner judges a lit view a match for the official client at the same pose, allowing for the
    differences listed under "Out of scope".

## Decisions (owner, 2026-10-24)

1. **One milestone in seven phases, each ending in something to try** (Question 1).

   | Phase | Steps | Ends with |
   | --- | --- | --- |
   | A. The fork | 1 to 3 | the official client's play packets captured, and client codecs |
   | B. The player | 4 to 7 | **owner checkpoint:** walk a fresh world |
   | C. The HUD and items | 8 and 9 | a hotbar with icons |
   | D. Building | 10 to 12 | **owner checkpoint:** build something |
   | E. Light | 13 and 14 | **owner checkpoint:** lit frames by day and night |
   | F. Entities and worlds | 15 and 16 | **owner checkpoint:** the world list |
   | G. Measure and accept | 17 and 18 | the acceptance session, then the tick |

   - **Why not two milestones:** PLAN.md's acceptance needs nearly all of it at once. Building needs
     the hotbar and the inventory, and "reload restores the world" needs the world list and a saved
     identity. The checkpoints give the owner what a split would.
   - **Each checkpoint is a go/no-go point** (owner, at approval).
     - **At each one,** a short report: what the phase built, what's left, and how long it took
       against the phases before it.
     - **Go:** the next phase starts.
     - **No-go,** if a phase overruns: M5 is ticked with what's done, and the rest becomes its own
       milestone in PLAN.md, with M5's acceptance split between the two. The split is recorded in
       Deviations.

2. **One `Game` holds the client's whole picture** (worth teaching).
   - **What:** a new `game.rs` with `Game`: the `World` (blocks, and now light), the entities, the
     local player, the inventory and the clock. `Game::apply(packet)` is the only way packets change
     any of it.
   - **Why:** PLAN.md's wall between networking and rendering grows past chunks. `Session` decodes
     and hands over; the renderer and the UI only read `Game` ("UI reads world state; it never
     reads packets directly", PLAN.md, UI).
   - `Session` keeps its stage machine and `run_until`. `session.world()` stays as a shortcut, so
     the headless checks don't change.

3. **The simulation steps at 20 Hz; frames draw it interpolated** (worth teaching).
   - **Each tick (50 ms):** read the held actions, step the local player's physics once, and send
     one `PlayerAuthInput` with the result. Ticks already run apart from frames (`app.rs:186-205`);
     now they also move the player.
   - **Each frame:** draw the player at `lerp(last tick, this tick, alpha)`, where alpha is how far
     the frame is into the next tick. Mouse look goes straight to the camera every frame, never
     through the tick.
   - **Why:** jump height, fall distance and breaking time are all defined per tick, at the rate
     the server ticks. Interpolation keeps frames smooth at 165 Hz while the simulation runs at 20.
     The price is the vanilla one: a movement key shows up to one tick later. Looking around has no
     added delay.
   - **After a stall,** at most three ticks run back to back, as `MAX_CATCH_UP` does now
     (`app.rs:62-64`).
   - **`--bench` keeps M4's camera:** its scripted flight bypasses physics and interpolation, so it
     stays the flight M6 compares (Decision 20).

4. **Prediction: the client moves itself, and Pumpkin takes the result; the server's corrections
   still win** (worth teaching; Finding 1).
   - **Pumpkin takes a Bedrock player's position as sent,** with no speed or collision check, and
     never sends `CorrectPlayerMovePrediction`. So the client's predicted position *is* the
     server's, and predicting means not waiting.
   - **The corrections the client obeys:**
     - `MovePlayer` for its own runtime id, a teleport or a respawn: it snaps there and stops;
     - `SetActorMotion` for its own id: knockback;
     - `CorrectPlayerMovePrediction`, if a server ever sends one;
     - `UpdateAbilities` (may fly, flying, no clip, instant build), `SetPlayerGameType`, and
       `UpdateAttributes` (movement speed, health and hunger).
   - **Honest reports:** the server takes on-ground from `VerticalCollision` with a falling
     `delta.y`, and sprinting, sneaking and flying from the start and stop flags. So each input
     carries the flags the official client sends in the same situation (Step 1 shows which), set
     from the physics' real collisions, with the real `delta` and `move_vec`.
   - **Waiting for terrain:** the player doesn't move until the chunks around it have arrived, so a
     join can't fall through the world before they land.
   - **The same trust reaches LAN guests:** Pumpkin also uses whatever item a client says it holds
     when placing (Finding 3). That's harmless for the host, but not for M7's guests, so PLAN.md's
     Risks now record it, for M7 to decide (owner, at approval).

5. **Physics: vanilla's rules on Pumpkin's data** (worth teaching).
   - **The rules, per tick:** gravity 0.08 and vertical drag 0.98. Ground friction is the block's
     slipperiness (0.6, ice 0.98) times 0.91, and air friction 0.91. Ground acceleration is
     speed × 0.216 / slipperiness³, and air acceleration 0.02. Speed is the `minecraft:movement`
     attribute (0.1), ×1.3 sprinting and ×0.3 sneaking. A jump is 0.42 × the block's jump
     multiplier, plus a forward push when sprinting.
   - **A Rust reference exists:** Pumpkin's own mob physics (`entity/living.rs:1531-1606`,
     `entity/mod.rs:1401`). It needs a server world, so the client writes a pure version, tested
     without a server, and checks its constants against Pumpkin's.
   - **The official client is the ground truth** (owner, at approval; worth teaching).
     - Step 1 records clean runs of each motion on flat ground: walking, sprinting and sneaking in a
       straight line, standing jumps, and a fall of ten blocks.
     - Each `PlayerAuthInput` carries the position the official client's own physics predicted
       for that tick. So a run is a trace: the inputs it read, and where they took it.
     - Our physics replays each trace's inputs from the same start, over a flat floor at the
       trace's height, and its positions must match tick by tick: the jump's peak and how long it
       takes, the three walking speeds, and the fall. Bedrock's rules may differ from Java's in
       places; where they do, Bedrock wins, and the change is recorded in Deviations.
   - **Collision:** the player is a 0.6 × 1.8 box with its eyes at 1.62, lower while sneaking.
     - It moves along y first, then along x and z, the larger first, each clipped by the collision
       boxes in the swept region.
     - Then it tries again 0.6 higher, to step onto slabs and stairs.
     - Sneaking stops at edges.
   - **Collision boxes come from `pumpkin-data`** (Finding 9), through M3's Bedrock-to-Java table.
   - **Liquids and climbing, basic:** water and lava slow and drag, and holding jump swims up.
     Ladders and vines cap the fall and let you climb.
   - **Modes:**
     - survival and adventure walk;
     - creative also flies: a double tap on jump toggles it, as on Bedrock. Jump and sneak rise and
       sink, and landing ends it;
     - spectator flies through blocks, like M4's camera.

6. **The gamemode is chosen when a world is created, creative first** (Question 2).
   - The create-world screen offers survival and creative. The settings file says which is
     selected first.
   - **Why creative first:** survival works in M5 (physics, health, hunger, breaking by tool and
     hardness, death and respawn), but without crafting or containers it can't get far. Those come
     in milestone 5b, straight after M5 (Question 3), which revisits the default.
   - **`--gamemode` stays,** for `--data-dir` runs that skip the world list. As in M4, a host the
     world has saved keeps its own mode.

7. **Blocks M4 drew as stand-in cubes are drawn as Pumpkin's own boxes** (Question 4; worth
   teaching).
   - **Why now:** physics collides with each block's real shape. A stair drawn as a cube but
     collided as a stair, or an open door drawn as a cube that you walk through, would look broken.
   - **The shape:** a new `Shape::Boxes`, built from the outline boxes of the block's Java state in
     `pumpkin-data` (Finding 9).
     - Each box face shows the block's texture for that face, cut by its position, as slabs already
       are (`mesher.rs:301-313`). Java's own models texture stairs, fences, torches and ladders the
       same way.
     - Boxes stay inside their block, so the vertex format holds them.
   - **Connections come with the block:** Bedrock's fence, pane and bar states carry their
     connections, and stairs their corners (Finding 10). Pumpkin's Java logic sets them, so the
     client draws and collides with what it's sent, and never works a connection out.
   - **One Bedrock id, one set of boxes, checked** (owner, at approval): several Java states can
     share a Bedrock id (M3's table keeps the lowest). A test walks every Bedrock id and checks that
     all the Java states sharing it have the same collision and outline boxes. It lists any that
     don't, so each is looked at before the shapes rely on them (Step 7).
   - **Still approximate, and counted:** rails are flat, beds and chests are boxes, and dripstone is
     a column. The stand-in census becomes a census of approximate shapes, and STATUS.md's "Special
     shapes to do" stays the to-do list.

8. **Aiming: a voxel ray against the outline boxes** (worth teaching).
   - **The ray** steps from block to block along the look direction (Amanatides and Woo's
     traversal), and tests each block's outline boxes. It yields the block, the face and the exact
     point hit, which the place packet needs.
   - **The reach is Pumpkin's:** 4.5 blocks in survival and 5 in creative
     (`entity/player.rs:4234-4240`). The server allows one block more (`:4242-4250`), so an edit in
     reach is never refused for distance.
   - **The outline:** the aimed block's outline boxes as thin dark lines, as on Bedrock. Line lists
     are core WebGPU.

9. **Breaking and placing are predicted; the server has the last word** (worth teaching; Findings 2
   to 4).
   - **Why predict:** the server handles input at the start of its next tick, and sends block
     changes at the end of one (Finding 4). An unpredicted edit would show 50 to 100 ms late, three
     to six frames at 60 Hz.
   - **Breaking:**
     - creative breaks at once, and again every few ticks while held (the capture shows the
       interval);
     - survival takes the server's time. Progress per tick is the tool's mining speed ÷ hardness ÷
       30, or ÷ 100 if the tool can't harvest the block (`block/mod.rs:528-542`), with the hardness
       and the tool rules from `pumpkin-data`. A crack overlay, from the install's
       `destroy_stage_0` to `_9`, shows the progress;
     - the packets are the ones the official client sends: block actions in `PlayerAuthInput`
       (start, continue each tick, predict at the end, abort). The server accepts a predicted break
       from 65% progress (`play/mod.rs:80`).
   - **Placing:** a right click sends `InventoryTransaction` with a use-item-on-block, as captured.
     For its prediction, the client works out the server's target: the face's neighbour, or the
     aimed block itself when it's replaceable, like grass.
   - **What's predicted:**
     - every break, to air, or to water where the block was waterlogged;
     - a place only when its result is certain: the item's block has a single state, like dirt,
       stone or planks (Question 11). Logs, stairs and furnaces depend on how they're placed, so
       those wait the one or two ticks for the server.
   - **Reconciling** (worth teaching): each predicted position goes into a table, with a deadline of
     half a second for now.
     - A server `UpdateBlock` for it is noted, not drawn. Some updates in flight are older than the
       prediction, and drawing them would flicker.
     - When the server's state equals the prediction, the entry goes.
     - At the deadline, the server's last state is drawn, whatever it is. A refused edit snaps back.
   - **An urgent lane:** the edited sub-chunk, and the one across a border it's on, mesh in the same
     frame, ahead of the budgeted queue. Today the queue is only re-sorted on a few triggers
     (`meshes.rs:239-255`), so during a join an edit could wait 100 ms. Sub-chunks whose light the
     edit changed follow through the normal queue.
   - **Measured:** click to drawn, and click to confirmed by the server, logged and summarised at
     the end of a session.

10. **Items and the inventory mirror the server** (Findings 5 and 8).
    - **Item names** come from the join's `ItemRegistry`, and the creative list from
      `CreativeContent`.
    - **Item to block:** item stacks don't say which block an item places (Finding 8), so the client
      goes from item to Java item, to Java block, to its default state, to its Bedrock id, all in
      `pumpkin-data`.
    - **Containers** come from `InventoryContent` and `InventorySlot`, with the server's stack ids,
      and the selected slot from `PlayerHotbar`.
    - **Changes are item stack requests,** as the official client sends: creative takes from the
      list, and survival takes, places and swaps between slots. The client applies its own request
      at once; the response confirms it, or undoes it.
    - **The hotbar:** 1 to 9 and the wheel select (`MobEquipment`), a middle click picks the aimed
      block (`BlockPickRequest`), and Q drops.
    - **Icons:**
      - block items are small three-face blocks, drawn from the terrain's texture array. Plants and
        other flat blocks show their texture flat, as on Bedrock;
      - other items use `items/*.json`'s icon, then an `item_texture.json` key of the same name,
        then `textures/items/<name>`, then a small alias table;
      - anything else draws the missing texture. A census at startup lists those items, like M4's
        stand-in census.

11. **Light: the client propagates sky and block light itself** (PLAN.md, Lighting; Question 6;
    worth teaching).
    - **What a block does to light** comes from `pumpkin-data`, through the Bedrock-to-Java table:
      how much it emits, and how much it dims light passing through (Finding 9). A block with water
      on its second layer dims as much as the stronger of the two.
    - **Storage:** per sub-chunk, sky and block light as 4-bit levels. A sub-chunk whose levels are
      all the same, like open sky or solid rock, keeps one value.
    - **The algorithm is vanilla's,** flood fills over queues:
      - block light spreads from emitters, losing at least 1 per block;
      - sky light falls straight down at 15 through clear blocks, then spreads the same way;
      - a change runs a decrease pass first, then refills from the edges. The removal half is the
        part worth teaching.
    - **Arrival and edits:** a new chunk is lit with its loaded neighbours' borders, and lights
      them in turn. A replaced chunk is relit the same way. An edit relights at once, before its
      sub-chunk is meshed.
    - **Scheduling:** on the main thread, nearest first, within 3 ms a frame. Meshing waits for a
      sub-chunk's light, and light changes mark meshes dirty.
      - **Stop rule:** if, in a dev build at radius 8, light keeps terrain unmeshed for more than a
        few seconds after a join's last chunk, or an edit's relight and remesh takes longer than a
        frame, stop and propose a fix, probably a worker thread.
    - **Checked against the server** (Finding 12): Pumpkin runs a vanilla-style light engine and
      keeps each chunk's light. A headless check compares ours with it, level for level, on every
      chunk whose neighbours are loaded, with re-checks like M3's world check.

12. **Drawing light: smooth lighting and ambient occlusion, then the time of day, the sky and fog**
    (Question 5).
    - **Smooth lighting, Bedrock's default:** each vertex takes the average light of the four cells
      around its corner, on the face's open side. Ambient occlusion darkens a corner by how many of
      the three cells around it are solid.
    - **No bigger vertex** (worth teaching): the sky and block levels fill the vertex's spare byte
      (`mesher.rs:62-77`), and the occlusion takes two of the face byte's three free bits. The GPU's
      vertex memory stays where M4 measured it.
    - **Corners need diagonal neighbours:** M4's neighbourhood leaves its edges and corners air
      (`mesher.rs:4-10`). Now it fills them, so a chunk is meshed only once all eight neighbours
      have arrived, and the view's unmeshed outer ring gets its corners too.
    - **A switch:** `smooth_lighting = false` in the settings gives each face the light of the cell
      it faces, as Bedrock's own setting does.
    - **Brightness:** a 16 × 16 light map, sky by block, worked out each frame from the time of day
      with Java's light curve. Bedrock's own curve lives in its engine, so the comparison will
      judge it.
    - **The time of day** comes from `SetTime`. Between packets the client advances it a tick at a
      time, unless the game rules stop it.
    - **The sky** is a colour by the time of day, and **fog** fades the terrain into it, ending just
      inside the meshed area. That hides the view's edge, and the dark band M4 saw over water
      (STATUS.md, Notes for M5). Under water, the fog is denser and blue.

13. **Entities live in `hecs`, are drawn a tick in the past, and are drawn as their boxes**
    (Questions 7 and 8; Finding 11).
    - **`hecs`, not `bevy_ecs`** (PLAN.md open question): a small ECS with no scheduler, which fits a
      hand-written loop. `bevy_ecs` brings systems, schedules and change detection that M5 wouldn't
      use, and much more to compile.
    - **Components:** the runtime id; the Bedrock identifier and its `pumpkin-data` entity type, for
      the box's size; the last server poses with their arrival times; the item, for item entities;
      and the name, for players.
    - **Interpolation** (worth teaching): each entity is drawn where it was one tick ago, plus a
      margin, between the two server poses around that moment. So a frame always has two poses to
      blend.
      - The margin is measured, not guessed: the log reports how far apart updates arrive. It
        starts at 75 ms.
    - **Drawn as boxes,** the stand-ins for M8's models: lit by the light where they stand, and
      coloured by kind (monster, animal, item, player, other). Item entities are small spinning
      icons.
    - **An identifier `pumpkin-data` doesn't know** gets a 1-block box, and the log counts them.

14. **The client's own UI: a retained tree, laid out the JSON UI way** (PLAN.md, UI; worth
    teaching).
    - **Retained:** each screen builds its tree once, and changes properties when the game does,
      such as a slot's item. Layout runs again only when something changed, or the window resized.
    - **Laid out like JSON UI:** anchors, offsets and sizes (pixels, percentages or fill), stack
      panels and grids. Then M10's JSON UI subset maps onto the same tree, and the framework doesn't
      have to change.
    - **Widgets:** panels (nine-sliced images), images, labels, buttons, item slots and grids,
      scroll views and text fields.
    - **From the install, at runtime:**
      - sprites from `textures/ui` and `textures/gui`, with their nine-slice sizes from the JSON
        beside each (Finding 16), packed into one atlas at startup and never written to disk;
      - text in the bitmap font `font/default8.png`, each glyph as wide as its pixels, with a drop
        shadow;
      - strings from `texts/en_US.lang` (PLAN.md: "No hardcoded strings"). The few strings vanilla
        lacks go in the client's own lang file, in the repo.
    - **UI scale:** automatic, the largest whole scale at which a 320 × 240 screen fits, as Java's
      GUI scale does. The settings file can fix it (PLAN.md, Accessibility).
    - **Drawn last,** in its own pass over the frame: textured quads, alpha-blended. Block icons use
      a second pipeline, which samples the terrain's texture array.
    - **Input:** while a screen is open, the cursor is free, and the screen gets the clicks, the
      wheel and the keys. While playing, the cursor is captured.

15. **Screens: the HUD, the inventory, the pause menu, the world list, death, and a command line**
    (Question 12).
    - **The HUD:** crosshair and hotbar, and in survival hearts, hunger, armour and air. The held
      item's name shows above the hotbar when it changes.
    - **The inventory** (E): creative has the creative list's four categories, with scrolling;
      survival has the 27 slots, the hotbar, armour and the off hand. No crafting grid: that's
      milestone 5b (Question 3).
    - **Pause** (Esc, or losing focus): Resume, and Save & Quit.
    - **The world list** (Decision 17).
    - **Death:** "You died!" and Respawn, with the packets the official client sends.
    - **A command line:** / or T opens a text field. Enter sends a command, or chat, and the
      server's replies show above the hotbar for a few seconds. It makes `/tp`, `/time` and
      `/gamemode` usable in our own client.

16. **A home folder holds the settings and a per-install identity** (STATUS.md, Notes for M5).
    - **`--home <dir>`,** `run/home` by default (gitignored), holds:
      - `settings.toml`;
      - `identity.toml`;
      - `worlds/`, with `backups/` and `deleted/` beside it.
    - **The settings file:** the name, the view radius, the field of view, the mouse sensitivity,
      the UI scale, smooth lighting, the first gamemode for new worlds, and the keymap.
      - **TOML,** through the `toml` crate, as Pumpkin's own config is.
      - **Missing fields** are filled in with defaults and written back. Unknown keys and bad
        values are logged and replaced, never a panic.
      - **Saved safely** (worth teaching): written to a temporary file, then renamed over the old
        one, so a crash mid-write can't leave half a file.
      - **Flags** (`--name`, `--radius`, `--gamemode`) override it for one run, without saving.
    - **The keymap** binds keys and mouse buttons by name. M4's defaults change where M5 needs the
      key: Ctrl sprints, Shift sneaks (and sinks when flying), and Esc pauses.
    - **The identity:** a random 16-digit `xid`, made on the first run. It replaces `login::DEV_XID`
      (`login.rs:17-21`).
      - The headless run uses it too, so a dev world's saved host becomes a new player once. That's
        the cost STATUS.md already warns of.

17. **Worlds: one folder each, a world list, and Save & Quit by starting afresh** (Question 10;
    Finding 13).
    - **A world is a folder,** `<home>/worlds/<id>/`, which is the embedded server's working
      directory for it: its `world/`, its `data/ops.json`, and so on. The id is the name made
      folder-safe, numbered if taken.
    - **`limonite-world.toml`** beside them holds the display name, the gamemode, the seed, and the
      dates created and last played. It's the client's own file.
    - **The list,** at startup, most recently played first: Play, Create, Copy and Delete.
      - **Create:** a name, survival or creative (Decision 6), and a seed, random if left empty.
        The server gets them as its seed and default gamemode.
      - **Copy:** to `<home>/backups/<id>-<time>/`, while the world isn't running.
      - **Delete:** after a confirmation, the folder moves to `<home>/deleted/`. Emptying that stays
        manual while the client is young.
    - **`--data-dir <dir>` skips the list** and opens that folder, as today, for the bench, the
      comparisons and the headless runs.
    - **Save & Quit:** the host leaves, and the server stops and saves, as closing the window does
      now. Then the process exits, and a new one opens on the world list.
      - **Why a new process:** Pumpkin's stop flags are process globals, and its stop token can't be
        reset once it has fired (Finding 13). So one process runs one server.
      - **How:** without `--data-dir` or `--headless`, the first process is a small supervisor. It
        runs the client as a child, and starts a new child each time one leaves for the world list.
        It never opens a window or a server itself, so a terminal or `cargo run` stays attached.
    - **When a child fails** (owner, at approval):
      - **A crash:** the supervisor starts a new child on the world list, with an error that says
        the game stopped unexpectedly, gives the exit status, and points to the log.
      - **A death at startup,** before the child's window is up: one is treated as a crash. Twice
        in a row, the supervisor stops with a message saying the client can't start, rather than
        loop.
      - **How it knows:** the child exits with one code for "back to the world list" and 0 for a
        normal quit. It tells the supervisor once its window is up, so a failure before that counts
        as a death at startup.

18. **The host is an operator, and the embedded server has no spawn protection** (PLAN.md open
    question; Question 9; Findings 3 and 7).
    - **Operator:** before the server starts, the world's `data/ops.json` gets the host at level 4,
      if it isn't there already. That's a world owner's power on Bedrock with cheats on: every
      command, and the abilities packet then calls the host an operator.
    - **The host's UUID** comes from pumpkin-auth's own claims parser, reading the token we send, as
      `login.rs`'s test already does. So it can't drift from the server's.
    - **The entry** uses `pumpkin-config`'s own `Op` type, so the file's format follows Pumpkin's.
    - **Spawn protection off** (`spawn_protection = 0` in `embedded::config`): Bedrock worlds have
      none, and Pumpkin's default of 16 blocks refuses a non-operator's building around spawn.
    - **StartGame's hard-coded 2 stays.** Our client reads the abilities instead. For M7's guests,
      StartGame would call everyone an operator, so that's a note for M7, and an upstream
      candidate.

19. **The fork changes only by client codecs, specified by a new capture** (Findings 14 and 15).
    - **The capture** (Step 1) repeats M2's: a temporary hook, never committed, records what the
      official client sends while the owner plays a short script. It's the spec for every packet
      the client sends.
    - **Readers** for about 25 clientbound packets: player state, entities and inventory. Most are a
      one-line `cfg_attr` derive beside the struct; about six need hand-written mirrors.
    - **Writers** for about a dozen serverbound packets. Two need hand-written mirrors, and the
      `PlayerAuthInput` writer gains its actions.
    - **Tests:** sent packets byte for byte against the capture; received ones by a round trip
      through Pumpkin's writer, with every field away from its default.
    - **Captures that identify the owner:** text and command packets carry the player's Xbox id or
      UUID. Like `SLogin`, they stay in `run/captures/`, with `#[ignore]`d golden tests; the
      committed tests use made-up values.
    - **No seam, and no game code changes.** Every change is in `pumpkin-protocol`, behind `client`.

20. **The M6 baseline stays M4's, and M5 records its own run beside it** (Question 14).
    - The bench flies the same path from the same template (`run/m4-template`).
    - But M5's frames also light, fog, and draw entities and the HUD. So Step 17 runs the bench
      again at radius 8, 16 and 32, and STATUS.md's "Notes for M6" gets both columns, for a
      like-for-like comparison.

21. **Capabilities and assets, as before.**
    - **The GPU:** still core WebGPU only: line lists, a 2D texture for the UI atlas, and alpha
      blending. No optional features.
    - **Assets** (CLAUDE.md; m4-plan Decision 15): sprites, the font, the strings and the item
      textures are read at runtime from the install, never copied into the repo. Screenshots stay
      in gitignored folders. The client's own lang file is ours.

22. **Dependencies** (Question 13). Checked on crates.io on 2026-10-24.
    - **New:** `hecs` 0.11.2, MIT or Apache-2.0, MSRV 1.81.
    - **Already in the lockfile through Pumpkin,** taken with Pumpkin's own specs: `toml` 1.1 (the
      settings), `rand` 0.10 (the identity) and `uuid` 1.28 (the operator entry).
    - **Not now:** `gilrs`, so gamepads wait (PLAN.md, Input; Question 15), and `egui`.
    - Each crate arrives with the first code that uses it.

**Fork changes in M5:** `pumpkin-protocol` only, behind `client`: a few dozen one-line derives
beside existing structs, about eight hand-written mirrors in `bedrock/client_codecs/`, and new
fixtures with their provenance. No seam, and no game code.

## Findings while planning (fork at `443b954`, client at `ea14b40`, Bedrock 1.26.52 installed)

Fork paths are under `pumpkin/crates/`, and client paths under `client/src/`.

1. **Pumpkin trusts a Bedrock player's movement.** `handle_player_auth_input`
   (`pumpkin-core/src/net/bedrock/play/player_auth_input.rs`):
   - ignores input until the client has loaded, and while the player is dead (`:13-20`);
   - takes on-ground from `VerticalCollision` with a negative `delta.y` (`:22-25`);
   - sets the position to the reported eye position minus the eye height, unchecked (`:27-47`);
   - updates sprinting, sneaking, crawling and flying from the start and stop flags (`:154-221`);
   - handles `block_actions`, and ignores the item interaction and item stack request that the
     same packet can carry (`:223-227`).
   - No code in `pumpkin-core` sends `CorrectPlayerMovePrediction`. A teleport sends `MovePlayer` in
     teleport mode (`entity/player.rs:4163`, Bedrock branch `:4209-4231`).
2. **Breaking** (`pumpkin-core/src/net/bedrock/play/player_action.rs`):
   - creative breaks on start (`:45-55`);
   - survival tracks progress with `calc_block_breaking` (`:68`; the formula at
     `pumpkin-core/src/block/mod.rs:528-542`);
   - a predicted break is accepted from 65% progress (`MIN_PREDICTED_BREAK_PROGRESS`,
     `play/mod.rs:80`). Otherwise the server sends `UpdateBlock` with the real state
     (`player_action.rs:201-203`);
   - abort stops mining (`:227-229`), and respawning is a player action too (`:233-241`).
   - Block actions inside `PlayerAuthInput` go through the same handler
     (`player_block_action.rs:5-22`).
3. **Placing** is `InventoryTransaction`'s use-item, click-block
   (`pumpkin-core/src/net/bedrock/play/inventory_action.rs:172-306`).
   - It tries the block's own use first, then the item's, then places (`:280-291`).
   - If the client's `item_in_hand` differs from the server's held item, the client's is used
     (`:202-209`).
   - **Spawn protection:** placing within 16 blocks of spawn is refused for players below level 4
     (`pumpkin-core/src/block/registry.rs:725-735`, `world/mod.rs:561-580`; the default of 16 at
     `pumpkin-config/src/lib.rs:225,248`). So today's host, with no operator entry, can't build
     near spawn.
4. **When the server answers.** It handles a player's packets at the start of that player's tick
   (`pumpkin-core/src/entity/player.rs:2717-2718`), and sends block changes once per world tick
   (`world/mod.rs:1528`, `:1719`). So an edit's `UpdateBlock` arrives one to two ticks after the
   click.
5. **The inventory.**
   - Item stack requests: creative take (`pumpkin-core/src/net/bedrock/play/item_stack_request.rs:37-76`),
     take and place (`:77-219`), swap (`:220-241`), drop (`:242-279`), destroy and consume
     (`:280-343`), and crafting (`:344-448`).
   - The hotbar slot is `MobEquipment` (`mob_equipment.rs:12-32`). Opening the inventory is
     `Interact`'s open-inventory, answered with `ContainerOpen` (`interaction.rs:5-18`).
   - The server syncs containers with `InventoryContent` and `InventorySlot`
     (`entity/player.rs:7726-7900`), and the selected slot with `PlayerHotbar` (`:7921-7930`).
6. **What a Bedrock player gets at join** (`pumpkin-core/src/world/mod.rs:2659-3147`):
   - `StartGame`, with server-authoritative block breaking on (`:2809`), and `permission_level: 2`
     behind a TODO (`:2754-2755`);
   - `ItemRegistry` (`:2843-2861`), `CreativeContent` (`:2863-2922`) and `CraftingData`;
   - the abilities (`:2934-2957`), and attributes: movement 0.1, gravity 0.08, air, health and
     hunger (`:2964-3031`).
   - Later, health and hunger come as `UpdateAttributes` (`entity/player.rs:4429-4460`), and the
     time as `SetTime` (`world/time.rs:120-131`, `:145-158`).
7. **Permission and identity.**
   - A player's level comes from `ops.json`, else from `default_op_level`
     (`pumpkin-core/src/entity/player.rs:790-799`; `data/op.rs:15-26`; the entry type at
     `pumpkin-config/src/op.rs:9-21`).
   - The abilities packet makes level 4 an operator, and everyone else a member
     (`entity/player.rs:3270-3284`).
   - A Bedrock player's UUID is an MD5 of `pocket-auth-1-xuid:<xid>`
     (`pumpkin-auth/src/jwt/mod.rs:500-533`).
8. **Items don't say which block they place.**
   - A network item stack takes its `block_runtime_id` from the mapping's `bedrock_block_state`
     (`pumpkin-protocol/src/bedrock/network_item.rs:348-370`), which the codegen always sets to 0
     (`pumpkin/tools/pumpkin-codegen/src/item.rs:1827`). Creative entries send 0 too
     (`pumpkin-core/src/world/mod.rs:2898-2915`).
   - `pumpkin-data` has the whole path: `JavaToBedrockItemMapping::from_bedrock`
     (`pumpkin-data/src/generated/item.rs:122944`), then `Block::from_item_id`
     (`generated/block.rs:598407`).
9. **`pumpkin-data` has what physics, light and breaking need:**
   - per Java state: emitted light, light opacity, hardness, and collision and outline boxes
     (`pumpkin-data/src/block_state.rs:14-38`, accessors `:207-245`; the boxes at
     `generated/block.rs:2287`);
   - per block: hardness, slipperiness, and the speed and jump multipliers (`blocks.rs:19-40`);
   - tools: `ToolImpl` and its rules (`data_component_impl/combat.rs:389-413`).
   - The client reaches them through its Bedrock-to-Java table (`client/src/world/mod.rs:277-353`),
     where the lowest Java state wins.
10. **Bedrock's block states carry connections and corners.** In the fork's palette
    (`pumpkin/assets/bedrock/block_states.nbt`):
    - `oak_fence`, `glass_pane` and `iron_bars` have 16 states each, keyed
      `minecraft:connection_east`, `_north`, `_south` and `_west`;
    - `oak_stairs` has 40, with `minecraft:corner`, `upside_down_bit` and `weirdo_direction`;
    - `cobblestone_wall` has 162, with `wall_connection_type_*`.
    - So a fence's arms and a stair's corner arrive in its id. The Bedrock-to-Java table should give
      back a Java state with the same arms and corner, so the same boxes; Step 7 tests it.
11. **Entities.**
    - `AddActor` carries the Bedrock identifier, such as `minecraft:cow`
      (`pumpkin-core/src/entity/mod.rs:405-441`). Players come as `AddPlayer`
      (`entity/player.rs:1125-1155`).
    - Every tick, `MoveActorDelta` sends each changed axis as an absolute `f32`, and the angles
      packed in bytes (`world/entity_tracker.rs:296-363`).
    - `SetActorMotion` follows velocity changes (`:247-262`), and `RemoveActor` removals
      (`:592-637`).
    - `pumpkin-data`'s `EntityType` has the width, height and eye height
      (`pumpkin-data/src/generated/entity_type.rs:12-34`), and `from_name` (`:7544`).
12. **The server has its own light.**
    - `pumpkin-world` runs a vanilla-style light engine: initial light during generation
      (`pumpkin-world/src/lighting/engine.rs:771-789`), and updates as blocks change
      (`lighting/runtime.rs`). Its default mode is full propagation (`pumpkin-config/src/lighting.rs`).
    - Each `ChunkData` keeps sky and block light per section (`pumpkin-world/src/chunk/mod.rs:78`,
      `:153-155`; `LightContainer::get` at `chunk/format/mod.rs:1377`). How those sections line up
      with the block sections is for Step 13 to check.
13. **Saving, and one server per process.**
    - Pumpkin autosaves every 6,000 ticks, five minutes (`pumpkin-config/src/world.rs:15-30`), and
      saves everything at shutdown (`pumpkin-core/src/lib.rs:520-553`). `Server::save_all` saves
      on demand (`server/mod.rs:585-614`).
    - `SHOULD_STOP`, `SERVER_IS_STOPPING` and `STOP_INTERRUPT` are process globals (`lib.rs:212-221`).
      The last is a `LazyLock<CancellationToken>`, which can't be reset once cancelled.
    - The world folder is `default_level_name` under the working directory
      (`pumpkin-config/src/lib.rs:212,243,255-258`), and the seed is `basic.seed` (`:187-188,231`).
14. **The client today.**
    - Ticks are counted apart from frames, and send the camera's pose (`app.rs:186-205`). The camera
      moves per frame (`app.rs:288-299`, `render/camera.rs:89-101`).
    - `Session` handles nine packet types (`session.rs:377-421`). Its input reports the camera's
      moves with fixed flags and no movement vector (`:511-557`).
    - The vertex has a spare byte, and three spare bits in the face byte (`render/mesher.rs:62-77`).
      The neighbourhood's edges and corners stay air (`:4-10`), and only side neighbours are
      required (`:160-172`).
    - Meshing re-sorts its queue only when the queue empties, when the camera changes sub-chunk, or
      100 ms after new work arrived (`render/meshes.rs:239-255`).
    - The keymap binds keys only (`input.rs:28-49`), `Settings` holds only the gamemode
      (`settings.rs:6-26`), and the identity is fixed (`login.rs:17-21`).
15. **The client codecs today** (`pumpkin-protocol/src/bedrock/client_codecs/`):
    - eight modules of hand-written mirrors (`mod.rs:18-25`). The `PlayerAuthInput` writer refuses
      actions (`player_auth_input.rs:17-29`), and the fixtures come from a session without a click
      (`fixtures/README.md`).
    - Of the clientbound packets M5 reads, most derive their writer, so one line adds the reader.
      `MovePlayer`, `MoveActorDelta`, `CreativeContent`, `ItemStackResponse`, and the actor
      metadata and synced properties that `AddActor` and `SetActorData` carry, are hand-written, so
      they need mirrors. Item stacks already read both ways (`network_item.rs:289-346`).
    - Of the serverbound packets, `InventoryTransaction` and `ItemStackRequest` have hand-written
      readers, so they need hand-written writers. The rest derive.
16. **The install has the HUD's and the menus' assets** (read at runtime; Bedrock 1.26.52):
    - `textures/ui`: `hotbar_0` to `_8` with start and end caps, `selected_hotbar_slot`, hearts,
      hunger, armour and air bubbles, and buttons and scroll bars. Nine-sliced sprites have a JSON
      beside them, such as `{"nineslice_size":1,"base_size":[4,4]}`;
    - `textures/gui/icons.png`, with the crosshair, and `textures/environment/destroy_stage_0` to
      `_9`;
    - `font/default8.png` and the `glyph_XX` pages;
    - `texts/en_US.lang`, loose, with the strings the menus need: `selectWorld.title`,
      `selectWorld.create`, `selectWorld.delete`, `menu.quit` ("Save & Quit"), `menu.returnToGame`,
      `container.inventory`, and `gameMode.creative` and `.survival`;
    - `textures/item_texture.json`, whose keys often hold variants (`axe`: wood to diamond). Only 44
      items have an `items/*.json` with an icon; the rest are mapped inside the engine. So item
      icons need a lookup order and a census (Decision 10).

## Step 1: Capture the official client's play packets (temporary, never committed; owner present)

1. **A temporary hook,** as in M2 Step 1: in `BedrockClient::process_batch`
   (`pumpkin-core/src/net/bedrock/mod.rs:616-645`), just before `handle_game_packet`. It writes every
   packet but the login (id 1) to `captures/<id>-<seq>.bin`, up to 5,000 per id.
2. **The capture server:** the stock fork at `443b954` plus the hook, built with STATUS.md's release
   command. It runs from `run/m5-capture/`, with M4's `pumpkin.toml` and a `data/ops.json` holding
   the owner at level 4.
3. **The owner plays a script** in Bedrock 1.26.52 on Fancy graphics, in about 30 minutes:
   1. in survival: stand, walk, sprint, sneak, jump, sprint-jump, and drop down three blocks;
   2. **the clean runs for the physics check** (Decision 5), on flat ground, each started from
      standing still and facing one way: walk straight for 5 s, sprint straight for 5 s, sneak
      straight for 5 s, and three standing jumps. Then build a 10-block pillar in creative,
      `/gamemode survival` at its top, and step off;
   3. `/give @s dirt 16`, `/give @s oak_planks 16` and `/give @s torch 4`;
   4. break dirt by hand to the end; start on stone and let go early;
   5. place dirt on the ground and against a wall, and a torch;
   6. press 1, 2 and 3, then scroll the wheel; drop one item with Q;
   7. open the inventory, move a stack, swap two, and close it;
   8. `/gamemode creative`; double-tap jump to fly, then rise, sink and land;
   9. break blocks, with one click and then held; take stone from the creative list into the
      hotbar; pick grass with a middle click; place blocks;
   10. `/gamemode survival`, `/kill`, and respawn;
   11. send a chat message, then `/time set noon`, and quit.
4. **Revert the hook** with `git -C pumpkin checkout -- crates/pumpkin-core/src/net/bedrock/mod.rs`.
   Confirm `git -C pumpkin status` is clean, then rebuild the stock server.
5. **Choose the fixtures,** one or two per packet and action, into `client_codecs/fixtures/`, and
   extend its README's provenance and table.
   - Text and command packets stay in `run/captures/m5-official/` (Decision 19).
   - **The physics traces:** the `PlayerAuthInput` packets of each clean run, in order, go into the
     client's own fixtures (`client/src/player/fixtures/`), with a README of their provenance and
     what each run shows. They carry no identity.
6. **Record in Results** which packets and flags the official client sends for each action. That's
   the spec for Steps 6, 9, 11 and 12.

## Step 2: Client codecs for play (fork, `pumpkin-protocol`)

1. **Player state, read:** `MovePlayer` (a mirror), `SetActorMotion`, `CorrectPlayerMovePrediction`,
   `UpdateAbilities`, `UpdateAttributes`, `SetPlayerGameType`, `SetHealth` and `SetTime`. `Respawn`
   already reads and writes.
   - **Commit (fork):** "Read player-state packets on the client side".
2. **Entities, read:** `AddActor`, `AddPlayer`, `AddItemActor`, `RemoveActor`, `MoveActorDelta` (a
   mirror), `MoveActorAbsolute`, `SetActorData` with its metadata (mirrors) and `TakeItemActor`.
   - **Commit (fork):** "Read entity packets on the client side".
3. **Inventory, read:** `ItemRegistry`, `CreativeContent` (a mirror), `InventoryContent`,
   `InventorySlot`, `PlayerHotbar`, `ItemStackResponse` (a mirror), `ContainerOpen` and
   `ContainerClose`.
   - **Commit (fork):** "Read inventory packets on the client side".
4. **The player's actions, written:** `PlayerAuthInput` with its block actions (and its item
   actions, if the capture has them), `InventoryTransaction` and `ItemStackRequest` (mirrors),
   `MobEquipment`, `Interact`, `PlayerAction`, `Animate`, `BlockPickRequest`, `CommandRequest` and
   `Text`.
   - The refusal test becomes a golden test of a captured block action.
   - **Commit (fork):** "Write the player's actions on the client side".
5. **Tests:** each written packet byte for byte against its fixture; each read packet by a round
   trip through Pumpkin's writer, with every field away from its default; the private fixtures
   through an environment variable, like `SLogin`'s.
6. **Fork checks, in `pumpkin/`:**
   - `cargo clippy -p pumpkin-protocol --all-targets`, plain and with `--features client`;
   - `cargo clippy -p pumpkin-core --all-targets --features pumpkin-protocol/client`;
   - `cargo test -p pumpkin-protocol --features client`.

## Step 3: The bump (superproject)

- **Commit:** "Bump pumpkin to the play codecs". No dependency changes yet.

## Step 4: The home folder, settings, identity, and the host as operator (client)

- **`home.rs`:** `--home` and its layout (Decision 16).
- **`settings.rs`:** the file, its fill-in and validation, the safe save, and the flags that
  override it.
- **`identity.rs`:** the per-install `xid`. `login::host_login` takes it, and `DEV_XID` goes.
- **`input.rs`:** bindings for keys, mouse buttons and the wheel, saved by name. The new actions are
  attack, use, pick, hotbar 1 to 9, inventory, drop, sneak, sprint, jump, pause and command.
- **`embedded.rs`:** spawn protection off; and before the server starts, the host's operator entry
  (Decision 18).
- **Tests:** the settings round trip, fill-in and bad values; the identity is stable across loads;
  the operator entry is added once and then kept; the host's UUID equals the one the server
  computes.
- **Commits:** "Keep settings and a per-install identity in a home folder"; "Make the host an
  operator of its worlds".

## Step 5: One `Game`, stepped at 20 Hz (client)

- **`game.rs`** (Decision 2), with `player/mod.rs` (the local player: runtime id, gamemode,
  abilities, attributes, health, hunger and air, and the last two ticks' poses) and `clock.rs` (the
  tick clock, alpha, and the time of day).
- **`Session`** hands every packet to `Game::apply`. That now applies `MovePlayer`, `SetActorMotion`,
  `CorrectPlayerMovePrediction`, `UpdateAbilities`, `SetPlayerGameType`, `UpdateAttributes`,
  `SetHealth`, `SetTime` and `GameRulesChanged`.
- **`app.rs`:** M4's flying moves the player each tick instead of the camera each frame. The camera
  draws it interpolated, and mouse look stays per frame (Decision 3). The bench keeps M4's
  per-frame flight.
- **A sixth headless check, "teleport":** after the walk, the check asks the server to move the
  host 5 blocks up (`Player::request_teleport`). The client must be there within a second, and
  report it in its next input.
- **Tests:** interpolation and alpha; the catch-up after a stall; each packet's effect.
- **Commit:** "Hold the client's picture in one Game, stepped at 20 Hz".

## Step 6: Walking, flying and the honest input (client)

- **`player/physics.rs`:** Decision 5's rules, as pure functions over a block lookup.
  **`player/movement.rs`:** the modes, and the double tap.
- **`PlayerAuthInput`** comes from the player's state: the flags, `delta`, `move_vec`, `raw_move`
  and the angles, as Step 1 recorded them for each situation.
- **Tests:**
  - a fall onto stone lands and reports `VerticalCollision`; a wall stops the player and reports
    `HorizontalCollision`;
  - a slab is stepped onto and a full block isn't; a jump peaks about 1.25 blocks up;
  - sprinting is 1.3 times walking; sneaking stops at an edge; ice slides;
  - water sinks, and jumping swims up; a ladder climbs;
  - a double tap toggles flight in creative only; a spectator passes through stone;
  - **the official client's traces** (Decision 5): each clean run from Step 1 replayed, compared
    tick by tick. The test reports the largest difference for each run, and fails beyond 0.001
    blocks. Results records the numbers, and Deviations any rule changed to match Bedrock.
- **Commits:** "Walk with gravity and collision"; "Fly in creative, and pass through blocks as a
  spectator".

## Step 7: Pumpkin's shapes for the stand-in cubes (client)

- **`blocks/shapes.rs`:** collision and outline boxes per Bedrock id, from `pumpkin-data` through
  the Bedrock-to-Java table. Physics, aiming and the mesher all read it.
- **`blocks/appearance.rs` and `render/mesher.rs`:** `Shape::Boxes` for every stand-in
  (Decision 7). A box face on the block's edge is hidden by an opaque neighbour, as a cube's face
  is; the others are always drawn.
- **The census** counts the blocks drawn as approximate boxes, by name.
- **Tests:**
  - a stair is two boxes, and an inner corner three; a fence with `connection_east` has an arm to
    the east; an open and a closed door; a torch; a ladder; the census;
  - **every Bedrock id** (owner, at approval): all the Java states that share it have the same
    collision and outline boxes. The test lists every id where they don't, with the states and
    their boxes, and fails. Each one is looked at with the owner; one that's explained (say, boxes
    that differ only where water sits) goes on the test's list of known exceptions, with its
    reason, so a new one still fails.
- **Owner checkpoint:** walk a fresh world in survival and in creative, over stairs and fences and
  through doors (about 10 minutes).
- **Commit:** "Draw Pumpkin's block shapes where M4 drew stand-in cubes".

## Step 7b: Bedrock's textures and shapes for the blocks Step 7 got wrong (client; added at checkpoint 1)

Added on 2026-10-30, when the owner's walk found the physics right and the special models broken
(Deviation 23).

- **The contact sheets** (`render/sheet.rs`, test-only): block states drawn through the real mesher,
  texture array and shader, with no window, into labelled PNGs in `run/sheets`, gitignored. They're
  the evidence before and after, and what a model is made against.
- **Part 1, the textures:** `blocks.json`'s six slots are often a bag of textures, and a key's
  variants are picked by state. Each block like that gets a rule: which key and variant each face
  or part shows, by state, and which way a block with a front faces.
  - **A test walks every block name Pumpkin can send.** A name with more than one texture, or a
    texture with variants, must have a rule or be on a list of those whose slots really are faces.
    A new block can't slip through.
- **Part 2, hand-made shapes:** `Shape::Model`, a block's boxes and planes as a table of parts,
  with their texture rectangles.
  - **Which way a model turns** comes from the block's Java state, which Pumpkin's table gives for
    each Bedrock id, the same state physics collides with.
  - **A test walks every Java state of each model family,** and checks that the model lies where
    Pumpkin's outline for that state does. So a door can't hang on the wrong side.
  - **Collision doesn't change:** physics and aiming keep Pumpkin's boxes.
- **Left for M8:** block entities (chest lids, beds, signs, banners, heads and pots), which Bedrock
  draws as entity models with entity textures. They stay boxes until then, but face the right way.
- **Owner checkpoint:** a dripstone cave, a village and a forest, beside the official client.

## Step 7c: Block models from the user's own Java jar (client; the owner's idea, 2026-10-30)

The owner pointed at Blockbench's Asset Browser plugin, which turns the jar's files into working
models of every block, and asked for the misc models to be encoded better, within the law, and checked
against it (Deviation 26).

- **The law, as read from Mojang's EULA and Usage Guidelines on 2026-10-30.** This is an engineer's
  reading: Mojang says it won't advise on a project, and to ask an attorney.
  - **Re-encoding changes nothing.** The guidelines name models among Mojang's assets, and the EULA
    keeps copies and derivatives Mojang's, down to one block's look. A tighter format is still a copy,
    and the GPL can't cover it. So nothing derived from the jar goes in the repo or the binary, generated
    or typed by hand.
  - **What's fine is what the plugin does, and what we already do with textures:** it ships no assets
    and reads the user's own copy on the user's machine. The EULA cares about giving copies to others,
    and the guidelines are relaxed about what people do for themselves.
  - **The copy has to come from Mojang,** because the EULA wants game downloads from a source it
    authorises: the launcher's folder, or a file the user fetched from Mojang. Never a third-party
    mirror such as mcasset.cloud, and never one as a dependency.
- **The compiler** (`blocks/java_models.rs`), at startup, in memory, for the states the palette has:
  jar → block state files → model files, parents followed → baked quads.
  - **Finding the jar:** `--java-jar`, then the launcher's `versions/26.3/26.3.jar`, then the copy
    Blockbench's plugin keeps. **With none, Step 7b's hand-made models draw,** so a jar makes the blocks
    exact but isn't needed to play.
  - **The encoding** (worth teaching): quads are shared between blocks that only differ by texture.
    A quad holds a slot, not a texture, so all oak and spruce fences use the same quads. Measured on
    26.3, for the states Bedrock has: 2.3 MB of JSON, and 72,917 quads if each model were baked apart,
    become 16,730. It takes 112 ms, so nothing needs writing to disk.
  - **Finer vertices:** the jar's models have half and quarter sixteenths and tilted planes, so a
    position goes from a sixteenth of a block to a 1,024th, and texture coordinates from a sixteenth
    to a 128th. The vertex stays 16 bytes.
- **Textures:** a model names Java's textures, and Bedrock's art comes first.
  - **Bedrock's where it fits:** the same name (or a short table of names), and the same see-through
    texels, which is how we know a texture's laid out for the same shape.
  - **Java's where it doesn't,** read from the jar: 26.3 has block models for beds and signs, and
    Bedrock only has entity atlases for those, so they stop being planks now, not at M8.
  - **One layer per picture:** two textures with the same pixels share a layer. Of the 831 textures the
    misc models use, 615 have the same pixels in the install.
- **What keeps its own path:** full cubes (`blocks.json` and the rules know them, overlays and all),
  liquids, bamboo, plants whose tip depends on the block beyond, and anything the jar draws nothing for:
  chests, shulker boxes, banners, heads, decorated pots and conduits still wait for M8. Tints stay ours:
  the jar says which faces take a tint, and the block's colour comes from Bedrock's maps.
- **Checks:**
  - unit tests on made-up model files of our own, so no test needs or holds Mojang's;
  - an ignored test over every block state, with `LIMONITE_JAVA_JAR`, that lists what breaks an
    assumption;
  - the contact sheets, with and without the jar;
  - the owner's asset browser as the reference: Blockbench's own code reads the same files, and a
    script compares what it builds with what we baked.
- **The wall:** what looks at the jar writes only to gitignored `run/`. Corrections to Step 7b's tables
  don't come from it.

## Step 7d: Animated textures, fluids that flow, and a debug world (client; the owner's ask, 2026-10-31)

The owner found everything animated standing still, water and lava included, lava's texture looking
irregular, and no easy way to check a given block, and suggested a debug world like Java's (Deviation 29).

- **Animated textures:** Bedrock's `flipbook_textures.json` says which textures are strips of frames,
  how fast they run and whether frames blend. A jar's textures say the same in `.mcmeta` files.
  - **One layer per texture, rewritten as it animates** (worth teaching). A layer per frame would be
    simpler to draw, but there are 1,209 frames and the array holds 2,048 layers. So each animated layer
    is rewritten from the CPU when its frame changes, 20 times a second at most, as Java does with its
    atlas. The bench says what that costs.
- **Fluids:**
  - **The irregular lava:** two causes to confirm on a sheet. `blocks.json` marks lava's top `isotropic`,
    so each block turned its texture at random; and flowing textures cover two blocks (`quad`), so a side
    showed a whole 32-texel frame on one block.
  - **Flow:** corners take their height from the four columns around them, so a flow slopes as it does
    in Java, and a flowing top shows the flowing texture, turned the way it runs. That needs a look at
    diagonal neighbours, so the neighbourhood fills its edges where there's liquid.
- **The debug world** (`--debug-world`): a flat world holding every Bedrock id Pumpkin can send, 15,953 of
  them, in a grid with a gap between blocks, as Java's debug world has. Added through Pumpkin's public
  API, with no change to the fork. Ticks that would change it are off: no random ticks, no mobs, no day
  and night.
  - **Going to a block:** `]` and `[` jump between block names, `.` and `,` between a name's states, and
    typing a name in the terminal jumps there. The title bar says what you're looking at.
  - **A fluid corner:** pools, flows and waterfalls of water and lava, set out by hand, since nothing in
    the grid flows.
- **Checks:** unit tests on made-up strips and fluids; sheets of fluids at several ticks; the bench with
  animation on, against without; and the headless run.

## Step 8: The UI, and the HUD (client)

- **`ui/`:** `atlas.rs` (sprites and nine-slices), `font.rs`, `lang.rs`, `tree.rs` and `layout.rs`,
  `render.rs` with `ui.wgsl`, and `input.rs` (the pointer and focus) (Decision 14).
- **`ui/screens/hud.rs`:** the crosshair and hotbar, hearts, hunger, armour and air
  (Decision 15). The slots stay empty until Step 9.
- **Startup log:** the sprites, glyph pages and strings loaded, and what's missing.
- **Tests:** anchors, stacks and grids; the nine-slice maths; glyph widths from a synthetic sheet;
  lang parsing (comments, tabs, missing keys); hit testing; the shader parses and validates (naga,
  as in M4).
- **Commits:** "Add the client's own UI: sprites, text, strings and layout"; "Draw the HUD".

## Step 9: Items, icons and the hotbar (client)

- **`inventory/items.rs`:** the item registry, item to block, and icon resolution, with its census
  (Decision 10). **`render/icons.rs`:** three-face block icons from the terrain's texture array.
- **`inventory/mod.rs`:** containers and the selected slot from the server; selecting (1 to 9, the
  wheel), picking (a middle click) and dropping (Q), with Step 1's packets.
- **The HUD's hotbar** shows icons and counts, and the held item's name from the lang file.
- **Tests:** item to block for dirt, an oak log, a poppy and a torch; the icon lookup order; the
  census; the wheel wraps around the hotbar.
- **Commits:** "Know the server's items, and draw their icons"; "Select, pick and drop from the
  hotbar".

## Step 10: Aim at blocks (client)

- **`edit/aim.rs`:** the voxel ray and the reach (Decision 8). **`render/outline.rs`:** the outline,
  as lines after the opaque pass.
- **Tests:** the near face is hit; air and liquids are passed through; a slab is hit at its top,
  half a block up; a fence post's box; nothing beyond the reach.
- **Commit:** "Aim at blocks, and outline the one in reach".

## Step 11: Break and place, predicted (client)

- **`edit/mod.rs`:** the break and place state machine, with its packets as captured (Decision 9).
  **`edit/predict.rs`:** the prediction table. **`render/meshes.rs`:** the urgent lane. The crack
  overlay draws in the blend pass.
- **The latency summary** at the end of a session (Decision 9).
- **A headless "edits" check** (`--headless --edits <n>`): the scripted host places and breaks n
  blocks near itself with the real packets, and the world check confirms that the server applied
  them. Step 16 adds the reload half.
- **Tests:**
  - the table: confirmed, refused after the deadline, and an older update ignored;
  - survival times: dirt by hand in 15 ticks, stone by hand in 150, and stone with a wooden pickaxe
    in 23;
  - the place target: against a face, and into replaceable grass.
- **Owner checkpoint:** build something in creative and in survival (about 10 minutes).
- **Commits:** "Break blocks, predicted"; "Place blocks, predicted where the result is certain".

## Step 12: The inventory screens (client)

- **`ui/screens/inventory.rs`:** creative and survival (Decision 15), opened and closed with Step 1's
  packets.
- **`inventory/requests.rs`:** an item stack request for each move, applied at once, and confirmed
  or undone by the response (Decision 10).
- **Tests:** each move builds the request the capture shows; a refused request is undone.
- **Commit:** "Open the inventory: creative and survival screens".

## Step 13: Light (client, CPU)

- **`world/light.rs`:** the storage, the flood fills, arrival and edits, and the budget
  (Decision 11). Light changes mark meshes dirty.
- **A "light matches server" headless check** (`checks.rs`, run like the world check), against
  `ChunkData.light_engine` (Finding 12).
- **Tests:** a torch in a closed room (15 at the torch, one less per block); sky light under a roof,
  and through a hole in it; a torch removed (the decrease pass); water dims; light crosses chunk
  borders; open sky stays one value per sub-chunk.
- **The stop rule** (Decision 11), with numbers from a fresh copy of `run/m4-dev` at radius 8, in a
  dev build and in release.
- **Commits:** "Compute sky and block light on the client"; "Check the client's light against the
  server's".

## Step 14: Draw the light (client)

- **`render/mesher.rs`:** smooth light and occlusion per vertex, packed into the spare bits, and the
  full neighbourhood (Decision 12).
- **`render/terrain.wgsl` and `render/sky.rs`:** the light map, the time of day, the sky's colour
  and fog.
- **Tests:** occlusion in a corner, along an edge and in the open; smooth-light averages; the vertex
  packing round trip; daylight at noon, sunset and midnight; the shader validates.
- **Owner checkpoint:** frames at noon, at sunset and at night, and a torch-lit cave; the water's
  dark band is gone (about 5 minutes).
- **Commits:** "Draw the terrain lit, with smooth lighting"; "Follow the time of day, with the sky's
  colour and fog".

## Step 15: Entities (client)

- **`entities/mod.rs`:** the `hecs` world, its packets, and interpolation (Decision 13).
  **`render/entities.rs`:** the boxes and the item icons.
- **Logged:** how far apart entity updates arrive, and the unknown identifiers.
- **Tests:** a pose halfway between two updates; the delay buffer; removal; an unknown identifier.
- **Commits:** "Track entities with hecs, and interpolate their motion"; "Draw entities as their
  boxes".

## Step 16: Worlds, pausing, death and commands (client)

- **`worlds.rs`** and **`ui/screens/worlds.rs`:** the world list, and create, copy and delete
  (Decision 17).
- **`main.rs`:** the supervisor (Decision 17).
- **`ui/screens/pause.rs`** and **`death.rs`:** pause, Save & Quit, and respawn.
- **`ui/screens/command.rs`:** the command line, and the server's replies.
- **The headless reload check** (`--headless --check-edits`): a second run joins the world of an
  `--edits` run, and checks the edited blocks, and the host's position and inventory.
- **Tests:**
  - folder ids; the metadata round trip; delete moves and copy copies;
  - the supervisor's choice, as a pure function of how the last children ended (Decision 17): back
    to the list; back to the list with an error after a crash; and a stop after two deaths at
    startup in a row, but not after a death at startup that follows a good run;
  - the supervisor's arguments for the next child.
- **Owner checkpoint:** the world list (about 5 minutes).
- **Commits:** "Pick, create and delete worlds from a world list"; "Pause, quit to the world list,
  and respawn"; "Type commands, and show the server's replies".

## Step 17: The M6 bench again (release numbers only)

- `--bench 30 --radius R` for R = 8, 16 and 32, each on a fresh copy of `run/m4-template`, run as M4
  ran it (STATUS.md, "Notes for M6").
- **Record** an M5 column beside M4's baseline, in Results and in STATUS.md.
- **Commit:** "Measure the M6 bench again, with M5's frames".

## Step 18: Verify, accept, tick

1. **Fork:** the Step 2 checks. `git -C pumpkin diff 443b954 --stat` lists only `pumpkin-protocol`
   files.
2. **Client:** `cargo test --workspace` and `cargo clippy --workspace --all-targets`.
   - **No Mojang files** (Decision 21): no `.tga` or `.brarchive` file is tracked, the only tracked
     image is `docs/readme/title.png`, and the only `.lang` file is the client's own.
3. **Headless,** with `--headless`: the five M3 checks, teleport, light matches server and edits;
   then the reload check.
4. **The acceptance session** (owner, about 30 minutes, a dev build and a fresh home):
   - create a world in the list, in survival first; walk, sprint, jump, swim and climb; die, and
     respawn;
   - then in creative: build a small house with stairs, a door, glass and torches, and break parts
     of it;
   - use the inventory in both modes; Save & Quit; reopen the world from the list, and find
     everything as it was left;
   - the latency summary and the frame times go in Results, and the owner judges "no visible input
     lag".
5. **The official-client comparison** (owner, about 15 minutes), with M4's procedure (STATUS.md),
   now at noon, at sunset, at night, and in a torch-lit cave: light, sky, fog and shapes. Ignore the
   differences listed under "Out of scope".
6. **Tick,** committed as "Tick Milestone 5":
   - **PLAN.md:** tick M5; answer the ECS and permission-level questions; the Lighting paragraph;
     gamepads deferred (Input); and check milestone 5b's scope against what M5 built. Milestone 5b
     and the Bedrock trust risk went in at approval;
   - **STATUS.md:** progress, the new dev setup (the home folder, the world list, settings, the
     controls and flags), Gotchas, Notes for M6, and the shapes to-do list;
   - **README.md:** the world list, the controls, the settings file and the flags;
   - **CLAUDE.md:** the next session's starting point; **this file:** Deviations and Results.
7. **Ask the owner** before pushing anything, or opening an upstream PR.

## Owner involvement

- **Approval:** done on 2026-10-24, with five amendments (Status, at the top).
- **Step 1:** the capture session (about 30 minutes). Keep Bedrock at 1.26.52 for the whole
  milestone (STATUS.md, Gotchas, "Exact protocol").
- **Checkpoints:** after Steps 7, 11, 14 and 16 (5 to 10 minutes each). Each is a go/no-go point
  (Decision 1): the owner hears what's done, what's left and how long it took, and decides whether
  to go on or to tick M5 and split the rest off.
- **Steps 18.4 and 18.5:** the acceptance session and the comparison (about 45 minutes).
- **Pushes and upstream PRs:** ask first.

## Verification (summary)

- **Unit tests,** with no GPU and no Mojang files: physics, shapes, aiming, prediction, light,
  interpolation, layout, fonts and strings, settings, the identity, worlds, the supervisor and
  requests.
- **Against the official client:** our physics replays its captured traces tick by tick (Step 6),
  and every Bedrock id's Java states agree on their boxes (Step 7).
- **Golden tests** of every packet the client sends, against the official client (Step 2).
- **Headless, end to end:** M3's five checks, plus teleport, light matches server, edits and
  reload.
- **Measured:** input-to-screen latency, frame times, the cost of light and meshing, and the bench
  (Step 17).
- **The real thing:** the owner's checkpoints, the acceptance session, and the official client at
  the same pose.

## Out of scope

- **Crafting and containers:** crafting tables, chests, furnaces and the other block screens. They
  are milestone 5b, straight after M5 (Question 3).
- **Combat and item use:** attacking entities, eating, bows, buckets and riding.
- **What entities look like** (M8): models, textures, animations and name tags. **Sounds** (M8)
  and **particles** (M10).
- **The sky's objects:** the sun, moon, stars, clouds and weather.
- **An options screen and a rebinding screen:** the settings file instead. **Gamepads** (`gilrs`).
- **Pausing the world** in the pause menu: that's the control API (a PLAN.md open question), with
  M7.
- **Fidelity beyond Pumpkin's data:** Bedrock's own light curve and block shapes, plant offsets,
  per-biome water and animated textures (STATUS.md, Notes for M5).
- **Languages other than `en_US`,** and chat beyond its last few lines.
- **Other dimensions and portals,** and **LAN** (M7).

## Questions and answers (owner, 2026-10-24)

The owner answered all fifteen as recommended, and asked for five changes, folded in above (Status,
at the top). Question 3's answer made PLAN.md's milestone 5b.

The questions as they were asked:

1. **Keep M5 as one milestone in phases** (Decision 1)? Recommended: yes.
   - **The alternative:** split it into 5a (the player, building, light and entities) and 5b (the
     UI, the inventory screens and worlds). But 5a would need a stopgap hotbar.
2. **Should new worlds start creative** (Decision 6)? Recommended: creative first, survival
   offered.
   - **The alternative:** survival first, as Bedrock does.
3. **Where do container screens go?** No milestone in PLAN.md has crafting, chests or furnaces, and
   survival can't get far without them. Recommended: add them to PLAN.md now as their own
   milestone, straight after M5, while M5's UI is fresh.
   - **The alternatives:** fold them into M5, which makes it bigger, or place them later.
4. **Draw the stand-in blocks as Pumpkin's boxes** (Decision 7)? Recommended: yes, so what you see
   is what you collide with.
   - **The alternative:** keep M4's cubes, which then disagree with collision.
5. **Smooth lighting and ambient occlusion now** (Decision 12)? Recommended: yes, as Bedrock's
   default look, with a setting for flat light.
   - **The alternative:** flat light in M5, and smooth light later.
6. **Light on the main thread within a budget, with a stop rule** (Decision 11)? Recommended: yes,
   as M4 did for meshing.
   - **The alternative:** a worker thread from the start.
7. **`hecs` 0.11.2 for the ECS** (Decision 13)? Recommended: yes.
   - **The alternative:** `bevy_ecs` 0.20.
8. **Draw entities as boxes until M8** (Decision 13)? Recommended: yes, so interpolation can be
   seen.
   - **The alternative:** draw none until M8.
9. **The host's permission** (Decision 18): an operator at level 4 through `ops.json`, spawn
   protection off, and StartGame's hard-coded 2 left for M7? Recommended: yes.
   - **The alternative:** a fork seam that makes every local player an operator.
10. **Back to the world list through a fresh process** (Decision 17)? Recommended: yes, with no fork
    change.
    - **The alternative:** a fork seam that resets Pumpkin's stop flags and token, about five files
      and an upstream candidate, so one process can run world after world.
11. **Predict a place only when its result is certain** (Decision 9)? Recommended: yes, measured,
    adding rules for any block family in which the owner sees lag.
    - **The alternative:** client-side placement rules for every block family now.
12. **A minimal command line, with the server's replies** (Decision 15)? Recommended: yes.
    - **The alternative:** leave commands to the official client.
13. **The capture session and the dependencies** (Decisions 19 and 22): about 25 minutes with
    Bedrock 1.26.52, and `hecs` 0.11.2, plus `toml`, `rand` and `uuid` from the lockfile?
    Recommended: yes.
14. **Keep M4's bench and baseline, and add an M5 run beside it** (Decision 20)? Recommended: yes.
15. **Leave gamepads and an options screen for later,** with the settings file meanwhile?
    Recommended: yes.

## Deviations (filled in during execution)

1. **The capture script was tightened for the physics runs** (with the owner present, 2026-10-24).
   - **A flat floor in the sky:** `/fill -8 199 0 8 199 40 stone`, with nothing near it, so every
     trace's floor is known exactly.
   - **Each clean run starts with a `/tp`:** to the same spot, facing along an axis. That fixes the
     start, and marks the run in the recording.
   - **Three more runs:** a walk at yaw 30 and one with W and D, which test the turning and the
     diagonal maths that straight runs can't; and a held sprint-jump.
   - **The 10-block fall by `/tp`** instead of a pillar: the same fall, from rest.
   - **Placing before breaking,** since the strip has no dirt.
   - **Water and a ladder,** added as an optional item, which the owner did.
2. **The hook kept up to 60,000 `PlayerAuthInput` packets, not 5,000.** At 20 a second, 5,000 is
   four minutes, the same reason as m2-plan Deviation 1. It also wrote `captures/index.csv`, each
   packet's place in the overall order and its arrival time, so the runs could be found between
   the commands.
3. **The first attempt was discarded** (2026-10-24). The owner switched to creative before the clean
   runs, where a quick double tap on Space starts flying, and the session ended there. The capture
   was recorded again from scratch, in a fresh world, on 2026-10-26. The first attempt is kept in
   `run/m5-capture-attempt1/` (gitignored).
4. **The creative-list take wasn't captured:** the inventory was only opened in survival. The owner
   moved it to Step 12, together with the official-client join on the merged fork (Deviation 7).
5. **A twelfth trace, creative flight** (`fly.bin`), from the script's item 8. It wasn't one of the
   planned clean runs, but it's the only ground truth for flying.
6. **Landing doesn't end creative flight on Bedrock** (Decision 5 says it does). In the trace the
   player lands while flying, and Space still rises at flight speed until a double tap sends
   `StopFlying`. Bedrock wins: flight ends only by a double tap (or the server).
7. **The fork merged upstream Pumpkin mid-milestone** (the owner's request, 2026-10-26).
   - **What:** upstream `master` `5e9101f`, 44 commits, merged into `limonite` (`845450b`), then a
     fix for upstream's new NBT reader (`f9badd8`), and the bump `a8f0271`. `docs/upstream.md` has
     the details. The owner also made syncs weekly, merged rather than rebased.
   - **Finding 13 no longer holds.** Upstream lets an embedded server stop without ending the
     process (Pumpkin-MC/Pumpkin#3771), and the client now embeds it that way. Decision 17's
     supervisor, a fresh process per world, is to be looked at again with the owner before Step 16.
     A child process still isolates a crash better.
   - **Step 18.1's check changes.** A diff against `443b954` now includes all of upstream's
     changes. Instead, M5's own fork commits must touch only `pumpkin-protocol`:
     `git -C pumpkin log --no-merges --name-only 443b954..limonite ^upstream/master`.
   - **`uuid` comes in at 1.29,** Pumpkin's spec since the merge (Decision 22 said 1.28).
8. **Three Pumpkin bugs, found by the capture, are reported upstream instead of fixed** (owner,
   2026-10-26). Each hits every Bedrock player, so our host too:
   - no fall damage: Pumpkin-MC/Pumpkin#3396, already reported;
   - block pick ignored: Pumpkin-MC/Pumpkin#4321, opened from the owner's account;
   - a survival placement costs two items: Pumpkin-MC/Pumpkin#4130, already reported.

   **For this plan:** survival falls don't hurt, and survival building uses stacks up twice as
   fast. Step 9's middle click sends the official client's packet, which the server ignores until
   upstream fixes it. The physics, the predictions and their tests don't change. A weekly sync
   brings each fix in when it lands.
9. **`ItemRegistry` needed a mirror, and `CreativeContent` an owned twin** (Step 2; Finding 15
   expected a derive for the first).
   - **Item data:** each item's component data is an NBT compound written with no length in front
     (`#[serial(no_prefix)]`), which no derived reader can handle. The mirror parses the compound
     to find where it ends, and keeps its bytes. It's tested on Pumpkin's whole registry, 2,076
     items.
   - **The creative inventory:** `CCreativeContent` borrows its groups and entries as slices, so
     nothing can be read into it. The client reads `client_codecs::CreativeContent` instead, an
     owned twin with the same fields.
10. **Three small fork edits beyond derives and mirrors,** all in `pumpkin-protocol` and behind
    nothing a server build sees differently:
    - `client_codecs` is `pub`, so the client can name the creative twin;
    - `network_item`'s user-data parser is `pub(crate)`, so the twin reads items the same way;
    - `Interact`'s `Action` gains `Clone` and `Copy`, behind `client`: its derived writer copies the
      value.
11. **Actor metadata sends each value's type twice,** as a `VarUInt` and then as a byte. That's
    Pumpkin's format since its update to 1.26.40 (upstream `f2444bc`). The mirror reads both and
    checks that they agree.
12. **The item stack request writer covers only what the client sends:** take, place, swap,
    drop, destroy, consume, create and the creative take. The others are refused, and for some of
    them Pumpkin's reader drops data, so no writer could match it.
    - **The legacy action byte** after each action's type repeats the type. The capture shows
      this for take, place and swap; the creative take is checked at Step 12's capture.
    - **Chat:** the client writes `Text` with the server's own writer, since Pumpkin sends chat to
      clients too. It matches the official client byte for byte.
13. **Four more actions than Step 4 lists** (Step 4):
    - `hotbar_next` and `hotbar_previous`, so the wheel has something to bind to. Scrolling
      towards you moves right, as in the official client.
    - **Chat and commands are two actions,** `chat` (T) and `command` (/), not one. Then the
      command line can open with a `/` already typed for `command`, as the official client's does.
    - `log_pose` (P) stays from M4.
14. **A sixth headless check, "operator"** (Step 4). It checks that the server knows the host by
    the UUID the client worked out, at permission level 4. So the UUID test runs end to end on
    every headless run, not only against a fixed value. With Step 5's teleport, that's seven
    checks.
15. **The settings' gamemode takes all four modes** (Step 4). The create-world screen will offer
    survival and creative (Decision 6), but `--data-dir` worlds read the same setting, and the
    flag already took all four.
16. **`GameRulesChanged` isn't applied** (Step 5). Pumpkin never sends it to a Bedrock player, its
    `StartGame` carries no rules (`rule_data` is empty), and the fork's `RuleValue` can't hold a
    value anyway.
    - **What it was for:** knowing whether the day moves. The time of day now counts on by itself
      between the server's `SetTime`s, which Pumpkin sends every 20 ticks.
    - **A day that stands still** shows as two `SetTime`s, a second apart, with the same time.
      After a `/time set` while the day is frozen, the clock runs for up to a second before the
      next `SetTime` stops it.
17. **Where Bedrock's physics isn't Java's** (Step 6; Decision 5 asks for these). The official
    client's traces showed:
    - **The push** is `move_vec`, already normalised, times 0.98 and the speed. So a diagonal is
      no faster than straight ahead (Java's is 2% faster), and sneaking's 0.3 is already in it.
    - **Water** is decided the way older Minecraft did. The player's box, 0.4 shorter at both
      ends, must touch water. Modern Java uses the water's surface height instead.
      - The `water_exit` trace leaves the water between feet heights 203.51 and 203.63, below a
        surface at 204.
    - **Ladders:** on one, a fall is capped at 0.2 a tick before the move (Java: 0.15). Pushing
      against it, or holding jump, ends the tick at exactly 0.2 up, after gravity, so the next
      tick climbs 0.2.
    - **Flying:** Space adds 0.15 before the move, as in Java. But Shift takes 0.22, not 0.15.
      With neither key, 0.375 of the vertical speed stays, before Java's 0.6 after the move.
      - The tick a flight starts or stops, the push follows the new state and the drag after the
        move the old one.
    - **Single precision:** positions (at eye height) and velocities round to `f32` each tick.
      Standing on something, the height rounds up: the official client rests at 201.62001 on a
      floor whose top is 200.
      - Without this, the replays climbed off a ladder, and landed in water, a tick early.
      - So collisions ignore overlaps under 1e-5, not Java's 1e-7.
    - **`VerticalCollision`** is set when the move was stopped vertically. On the first tick after
      a teleport, the official client also sets it for a player standing still on the ground.
    - **`Sprinting`** (20) isn't the sprint state. It's sent while the sprint key is down and the
      player is, or just was, sprinting. With the key up the player keeps sprinting, unflagged,
      until forward is let go.
    - **Not in any trace, so Java's rule stands, untested:**
      - flying sideways (0.05 × 0.98, friction 0.91, twice as fast sprinting);
      - lava;
      - stopping a sprint at a wall;
      - the ladder's sideways limit;
      - the 10-tick delay between held jumps;
      - the double tap's window (7 ticks; the trace's taps were 4 apart).
18. **Step 6 started `blocks/shapes.rs`,** with the collision boxes physics walks on. Step 7 adds
    the outline boxes and draws them.
19. **Step 6 is one commit for walking and flying,** not two: the flight rules are part of the
    same tick, and `fly.bin` replays with the other traces. The collision boxes are a commit of
    their own.
20. **The scripted walk stays a script** (Step 6). The headless session still moves 0.2 a tick past
    physics, as M2's transport check did: its world is generated terrain, where a real walk can
    meet a hill. Standing still, and falling after the teleport check, go through physics.
21. **Shapes take the Java state nearest its block's default** (Step 7), not M3's lowest, for the
    184 Bedrock ids whose Java states differ in their boxes (`BlockTable::shape_state`).
    - **The reason:** the lowest piston state is pushed out (Java orders `extended` true first), so
      every piston would have been drawn, and collided with, a quarter short.
    - **What doesn't change:** map colours and names still use the lowest state.
22. **The consistency check's exceptions are explained in the test, not yet with the owner**
    (Step 7). The plan has each one looked at with the owner. All 184 come from 7 reasons, listed in
    Results, and they're in the test with their reasons, so a new one still fails. They're for the
    owner to confirm at the checkpoint.
23. **A new Step 7b, before Step 8** (owner, at checkpoint 1, 2026-10-30).
    - **The walk:** the physics felt right. But "most miscellaneous models (like dripstone, doors,
      leaf litter, chests)" were bugged.
    - **What the plan had wrong:** Decision 7 had "each box face shows the block's texture for that
      face". For many blocks `blocks.json`'s slots aren't faces, but a bag of textures Bedrock picks
      from by state, and a key's variants are picked by state too. And Pumpkin's outline boxes are
      shapes for aiming, not models: a fence's outline is a solid wall, and a torch's a hollow tube.
    - **What the plan said to leave:** "Fidelity beyond Pumpkin's data" was out of scope. The owner
      chose to bring Bedrock's shapes in, all but the block entities, which wait for M8.
24. **Step 7b reached further than the blocks the owner named** (Step 7b). The same two causes broke
    more than the 493 names Step 7 drew as boxes:
    - **Cubes with a front:** furnaces, barrels, hives, dispensers, observers, pistons and the like
      never turned, and an uncarved pumpkin showed a carved face. M4 had these wrong too.
    - **Textures by state on plain cubes and crosses:** huge mushrooms showed pores all over, every
      crop showed its first stage, and farmland was always wet.
    - **What's next door:** kelp and vines have a tip that depends on the block beyond, and grass
      under snow has a snowy side.
25. **Vertex positions are signed** (Step 7b), not the unsigned sixteenths of m4-plan Decision 6, so a
    model can reach outside its block as Java's do. A crop reaches a sixteenth below itself, to meet
    the farmland. The vertex is still 16 bytes.

26. **A new Step 7c, models from the user's jar** (owner, 2026-10-30). Step 7b wrote 60-odd models by
    hand because the install has no geometry (m4-plan Finding 8), and Step 7b's Deviation 23 kept block
    entities for M8. The owner pointed out that the Java jar has every model and asked for a better
    encoding within the law. That adds a second, optional source of assets beside the Bedrock install,
    read the same way: at runtime, from the user's copy, and never bundled.

27. **The Java state nearest its default now stands for every Bedrock id** (Step 7c), not only those
    whose boxes differ (Deviation 21). Laying the jar's models beside Blockbench's showed every repeater
    locked: Bedrock doesn't keep `locked`, and Java numbers `true` first, so the lowest state had it. No
    box changes, since states that differ in boxes already took this route.
28. **With a jar, beds and signs don't wait for M8** (Step 7c). Java 26.3 draws them as block models,
    so Step 7b's list for M8 shrinks to what the jar has nothing for: chests, shulker boxes, banners,
    heads, decorated pots, conduits and copper golem statues. A bed's colour still waits, as Bedrock
    keeps it in the block actor, so every bed is white until then.

29. **Animation comes before M8, and a new Step 7d holds it** (owner, 2026-10-31). The textures code put
    animation in M8, and PLAN.md left fluids as flat steps. The owner saw both on the walk, and asked
    for them after Step 7b, along with a debug world, which nothing in the plan had.

30. **The UI blends in gamma, through a second view of the frame** (Step 8). Decision 14 said only
    alpha-blended. In the frame's own sRGB view, blending happens in linear light, and the hotbar's
    half-transparent slots come out visibly paler than the art means. The second view needs
    `SURFACE_VIEW_FORMATS`, a capability that's asked for (Decision 21). Without it, the shader converts
    to linear, and the slots are pale.
31. **A third pipeline, for the crosshair** (Step 8), beside Decision 14's second for block icons. It
    inverts what's behind it, as Java's does, and as the official client's looks. Nobody has checked
    that side by side yet.
32. **Some of Decision 14's widgets wait for their screens** (Step 8): buttons, item slots, scroll views,
    text fields, and keyboard focus, which only Step 16's command line needs. So `ui/input.rs` has only
    the pointer. Each arrives with the first thing that uses it, as crates do (Decision 22).
33. **Breath isn't an attribute, and armour isn't sent** (Step 8). Step 5 read `minecraft:air`, but
    Bedrock has none: Pumpkin sends `AIR_SUPPLY` in `SetActorData`, so the client's air never moved, and
    `Game` now reads that. Bedrock works armour out from what's worn, so its row waits for Step 9.
34. **The font's cells are 8 × 12, not 8 × 8** (Step 8), and start 3 units above the line, as
    `font/default8.json` says. Finding 16 hadn't noticed the file. Accented letters come from the glyph
    pages, a little thinner, rather than `default8`'s own further down its sheet.
35. **A label above the hotbar came early** (Step 8). It's the one the held item's name will use (Decision
    15), and the debug world says what you're looking at with it for now.
36. **Screenshots with no window** (Step 8; the owner's ask, 2026-11-01): `client/src/shots.rs`, an ignored
    test that runs the real server, client and renderer, and saves frames to `run/screenshots`.

37. **A block's icon is its real mesh** (Step 9), not the three faces of Decision 10. The world's mesher
    already handles every shape, so an icon meshes the block alone, turns the quads to the inventory's
    angle, and flattens them, back to front, with no depth buffer. Stairs look like stairs, and it's less code
    than three faces and a list of exceptions.
38. **Picking waits for Step 10** (Step 9). A middle click picks the block aimed at, and nothing aims yet.
39. **Armour waits for Step 12, or for Pumpkin** (Step 9), and not for Step 9 as Deviation 33 said. Container
    0 is only the 36 main slots, and Pumpkin's Bedrock path sends no armour slots or off hand. The client can
    know what it put there itself once the inventory screen moves things. It's on the upstream list.
40. **A position never rounds into a box** (the owner's report, 2026-11-01). Step 6 rounded each tick's position
    to the nearest `f32`, as the official client keeps it, and forgave overlaps under 0.00001. That held where the
    traces were recorded, within 40 blocks of the origin on x and z, but an `f32`'s steps grow with distance. Now,
    where the nearest would put the box into something, the position takes the next one back. Nothing else changes:
    the traces replay as they did.

## Results (filled in during execution)

### Step 1: what the official client sends (2026-10-26)

**The capture:** 9,594 serverbound packets in 7.2 minutes, from Bedrock 1.26.52. It's in
`run/captures/m5-official/` (gitignored), with a decoded timeline beside it. Every packet was read
by Pumpkin's own reader with no bytes left over.

| Id | Packet | Count |
| --- | --- | --- |
| 144 | `PlayerAuthInput` | 8,513 |
| 44 | `Animate` | 499 |
| 135 | `ClientCacheBlobStatus` | 341 |
| 31 | `MobEquipment` | 57 |
| 30 | `InventoryTransaction` | 50 |
| 36 | `PlayerAction` | 42 |
| 33 | `Interact` | 36 |
| 77 | `CommandRequest` | 22 |
| 34 | `BlockPickRequest` | 17 |
| 147 | `ItemStackRequest` | 5 |
| 8, 312 | `ResourcePackClientResponse`, `ServerboundLoadingScreen` | 2 each |
| 9, 45, 47, 69, 113, 129, 193 | `Text`, `Respawn`, `ContainerClose`, `RequestChunkRadius`, `SetLocalPlayerAsInitialized`, `ClientCacheStatus`, `RequestNetworkSettings` | 1 each |

**The spec for Steps 6, 9, 11 and 12:**

- **Each tick, `PlayerAuthInput`'s flags:**
  - always 48 (`BlockBreakingDelayEnabled`); on the ground 50 (`VerticalCollision`), with `delta.y`
    -0.0784; against a wall 49 (`HorizontalCollision`);
  - W, S, A and D are 10, 11, 12 and 13. `move_vec` and `raw_move` are -1, 0 or 1 per axis, with a
    diagonal at 0.7071, and `analog_move` stays 0 on a keyboard;
  - **sprint:** 25 (`StartSprinting`) once, then 4 and 20 each tick, and 26 to stop;
  - **sneak:** 27 once, then 8, 9, 17 and 64 each tick, and 28 to stop. 63 and 62 mark the key's
    press and release;
  - **jump:** 3, 6, 16 and 61 while Space is held; 60 on the press and 59 on the release. 31
    (`StartJumping`) comes on the tick the jump starts: the press, on the ground, or the tick after
    landing with Space held;
  - **fly:** 42 (`StartFlying`) on the second tap, 43 (`StopFlying`) on the next double tap. Space
    and Shift rise and sink with the jump and sneak flags;
  - **teleported:** 37 (`HandledTeleport`) on the first input after the server moves the player,
    with the new position and a `delta` of 0;
  - 35 (`PerformBlockActions`) whenever the packet carries block actions. Water needs no flag of its
    own: swimming up is the jump flags, and `StartSwimming` needs sprinting.
- **Movement numbers** (the traces, below): walking 0.21586 blocks a tick, sprinting 0.28062,
  sneaking 0.06476; a jump peaks 1.25224 up; falling follows `(v - 0.08) × 0.98`; water
  `0.8 v - 0.005`, with 0.04 more a tick while rising; a ladder climbs at 0.2. All but the ladder's
  fall cap match Java.
- **Breaking:**
  - **survival:** `StartDestroyBlock` once in `PlayerAuthInput`'s block actions; nothing per tick;
    then `ContinueDestroyBlock` and `PredictDestroyBlock` together on the tick the block breaks
    (dirt by hand: 14 ticks after the start). Moving onto another block while holding sends
    `ContinueDestroyBlock` for it, and letting go sends `AbortDestroyBlock` with face 0;
  - **creative:** `PlayerAction` `CreativeDestroyBlock`, and `StartDestroyBlock` with
    `PredictDestroyBlock` in the same tick's input. Held, it repeats on the next block every 5
    ticks (once 4) with `ContinueDestroyBlock`;
  - **the swing:** `Animate` `SwingArm`, source `Mine`, about once a frame while the button is held.
    Our client sends one a tick.
- **Placing,** five packets a click:
  1. `PlayerAction` `StartItemUseOn`, with the clicked block, the placed position and the face;
  2. `Animate` `SwingArm`, source `Build`;
  3. `InventoryTransaction`, use item, click block (action 0, trigger 1, client prediction 1), with
     the hotbar slot's predicted change as a legacy action (an even negative id, container 29);
  4. `InventoryTransaction`, use item, click air (action 1, face 255), with no actions;
  5. `PlayerAction` `StopItemUseOn`, a few ticks later, when the button comes up.
- **The hotbar:** `MobEquipment` on every change, keys and wheel alike, often after an `Interact`
  `InteractUpdate` with target 0. Q is a normal `InventoryTransaction`: the slot's change and a
  world-source drop.
- **The inventory:** E sends `Interact` `OpenInventory` (target: the player's own runtime id).
  - Each move is an `ItemStackRequest` with a falling odd id (-3, -5, -7 and on): `Take` to the
    cursor, `Place` from it, `Swap` with it.
  - Closing sends `ContainerClose` with container 0 and type 247.
- **Block pick:** `BlockPickRequest` with `max_slots` 9; `with_data` is true on the first and
  false after (Deviation 8).
- **Death:** an `InventoryTransaction` of type mismatch after dying. Then Respawn sends `Respawn`
  (`ClientReadyToSpawn`) and `PlayerAction` `Respawn` (face -1), and the next input has
  `HandledTeleport` at the world spawn.
- **Commands and chat:** `CommandRequest` (origin `player`, version `latest`) and `Text` (`Chat`).
  Both identify the player, so their fixtures stay private (Decision 19).
- **Not needed by our client:** `ClientCacheStatus` and `ClientCacheBlobStatus` (the official
  client takes chunks from a blob cache; ours never asks for it), `ServerboundLoadingScreen`, and
  the login packets M2 covers.

**The fixtures:**
- **Codecs:** 23 new files in `client_codecs/fixtures/`, listed in its README.
- **Physics:** 12 traces in `client/src/player/fixtures/`, 1,256 ticks and 143 KB, listed in that
  README with each run's numbers.

### Steps 2 and 3: the play codecs (2026-10-26)

- **Readers, behind `client`,** for 24 packets the server sends:
  - **player state:** `MovePlayer` (a mirror), `SetActorMotion`, `CorrectPlayerMovePrediction`,
    `UpdateAbilities`, `UpdateAttributes`, `SetPlayerGameType`, `SetHealth` and `SetTime`;
  - **entities:** `AddActor`, `AddPlayer`, `AddItemActor`, `RemoveActor`, `MoveActorAbsolute`,
    `TakeItemActor`, `SetActorData`, and `MoveActorDelta` (a mirror). The actor metadata and the
    synced properties inside them are mirrors too;
  - **inventory:** `ItemRegistry` and `CreativeContent` (Deviation 9), `ItemStackResponse` (a
    mirror), `InventoryContent`, `InventorySlot`, `PlayerHotbar`, `ContainerOpen` and
    `ContainerClose`.
- **Writers** for the 12 packets the client sends:
  - `PlayerAuthInput` with its block actions; `InventoryTransaction` and `ItemStackRequest`
    (mirrors);
  - `MobEquipment`, `Interact`, `PlayerAction`, `BlockPickRequest` and `CommandRequest`, derived;
  - `Animate`, `Respawn`, `Text` and `ContainerClose`, which already wrote.
- **Tests:** every written packet with a fixture matches the official client byte for byte: 23
  fixtures, plus the command and the chat through `LIMONITE_M5_CAPTURES`. Every read packet
  round-trips through Pumpkin's writer with every field away from its default.
  - `pumpkin-protocol` passes 208 tests with `client`, up from 150, with 3 ignored. Those three,
    the login, the command and the chat, pass locally against the private captures.
  - Clippy is clean in all five configurations, and `net::bedrock` and the palette still pass.
- **The fork's diff:** M5's five fork commits (`f9badd8`, `77e0ac2`, `12ac8ba`, `2f4dff2` and
  `15745d6`) touch 68 files, all in `crates/pumpkin-protocol` (Deviation 7's check).
- **Step 3, the bump** (`3429125`): the client builds unchanged, its 75 and 26 tests pass, and
  `--headless` gives five PASS lines.

### Step 4: the home folder, settings, identity and the host as operator (2026-10-26)

- **Commits:** `2fe3859` "Keep settings and a per-install identity in a home folder", and
  `09cdbc3` "Make the host an operator of its worlds".
- **`--home`,** `run/home` by default: the first run writes `settings.toml` and `identity.toml`
  there. `worlds/`, `backups/` and `deleted/` come with Step 16's world list.
- **The settings file:**
  - **What it holds, and the limits:**
    - the name: 1 to 16 characters, none of them control characters. That's the Xbox gamertag
      limit. Pumpkin doesn't check Bedrock names at all;
    - the radius: 2 to 32;
    - the field of view: vertical, 30 to 110, default 70 (M4's);
    - the mouse sensitivity: a multiple of M4's 0.15 degrees a count, 0.1 to 10;
    - the UI scale: `"auto"` or 1 to 8;
    - smooth lighting;
    - the gamemode: creative by default;
    - the keymap, as a `[keys]` table.
  - **Reading:** each setting is read on its own. A missing one is filled in; a bad one, or an
    unknown key, is logged and replaced. Then the whole file is rewritten, with a comment on every
    setting.
    - A good file is never rewritten, so the player's own comments stay.
    - A file that isn't TOML is kept as `settings.toml.broken`, and the defaults replace it.
  - **Flags:** `--name`, `--radius` and `--gamemode` override it for one run. `--name` is checked
    by the file's rule.
- **The keymap:**
  - **Keys** are named by their W3C `code`, which is what winit calls its physical keys. `KeyW` is
    the key where QWERTY has W, whatever the layout prints on it.
  - **The mouse:** `MouseLeft` to `MouseForward`, and `WheelUp` and `WheelDown` for the wheel.
  - **Several or none:** a list binds several buttons to an action, and `[]` binds none.
  - **The defaults:** Ctrl sprints, Shift sneaks, and Esc pauses. Until the pause menu, Esc
    releases the cursor.
  - **The mouse in the window:** while the cursor is free, a left click only captures it. A
    button's release always counts, so nothing stays held after a pause.
- **The identity:** 16 random digits, the first not a 0.
  - **A damaged `identity.toml` stops the client,** with a message saying how to fix it, rather
    than quietly becoming a new player.
  - **M4's dev host:** writing `xid = "1000000000000001"` there brings it back.
- **The host as operator:**
  - **The UUID:** the host's UUID for M4's dev `xid` is `eb9cf2d5-8d1a-3340-b297-a4713ed878e4`. A
    test pins that value, which is the server's own player file for that host.
  - **The entry:** on `run/client`, the first run added the host to `data/ops.json` at level 4, in
    Pumpkin's format. The second left the file byte for byte as it was.
  - **The name** in the entry is the one Pumpkin's login builds: the prefix, and spaces made
    underscores.
  - **Spawn protection** is 0.
- **Tests:**
  - the client's tests went from 75 to 97: settings 7, identity 3, the keymap 5 more, home 2,
    the operator list 4, and the host's UUID 1;
  - clippy is clean.
- **Headless:**
  - **A fresh home:** six PASS lines. The new identity became one new player file in
    `run/client`, and M4's dev player file wasn't touched.
  - **A second run with `--radius 4`:** it asked for radius 4, and reused the same player file.
    Neither file in the home folder changed.
  - **Flags:** a bad `--name` and `--radius 1` are refused with exit code 2.

### Step 5: one `Game`, stepped at 20 Hz (2026-10-26)

- **Commit:** `5b0f117` "Hold the client's picture in one Game, stepped at 20 Hz".
- **`game.rs`:** `Game` holds the world, the local player and the time of day.
  - `Session` keeps the login, the stages and the input, and hands every other packet to
    `Game::apply`.
  - It applies `MovePlayer`, `SetActorMotion`, `CorrectPlayerMovePrediction`, `UpdateAbilities`,
    `SetPlayerGameType`, `UpdateAttributes`, `SetHealth` and `SetTime`, besides the world's three.
    `GameRulesChanged` isn't among them (Deviation 16).
  - A packet about another runtime id is left for Step 15.
- **`player/mod.rs`:** the local player's runtime and unique ids, gamemode, abilities,
  attributes, velocity, and this tick's and the last tick's poses.
  - **Abilities:** each ability comes from the last of the server's layers that sets it. Pumpkin
    sends a base layer, and for a spectator a second layer that turns flying on.
  - **A teleport** (`MovePlayer` in teleport, reset or normal mode) snaps the player there, with
    nothing drawn in between, and holds it still for one tick. The next input carries
    `HandledTeleport`, the new position and a `delta` of 0, as Step 1 recorded.
  - **The camera** takes the teleport's angles before the next tick reads them.
- **`clock.rs`:**
  - **The tick schedule:** at most three ticks catch up after a stall.
  - **Alpha:** it never draws past the newest pose.
  - **The time of day:** it counts on by itself between `SetTime`s.
- **The window:**
  - **Movement:** M4's flight moves the player once a tick, through `Motion::Fly`.
  - **Frames:** each draws the camera at `lerp(last tick's eyes, this tick's, alpha)`, and the mouse
    still turns it every frame.
  - **The bench** still flies its camera frame by frame, and the player follows it (`Motion::To`).
- **The scripted session** still moves by its fixed steps (`Motion::To`) until Step 6 gives it
  physics. Its `PlayerAuthInput`s are M4's, but now come from the player's state.
- **The "teleport" check** (headless, the seventh):
  - after the walk, the server moves the host 5 blocks up with `Player::request_teleport`;
  - the next input reported it 50 ms later, one tick, with the eyes exactly where the server put
    them and a `delta` of 0.
- **Runs:**
  - **Headless:** all seven checks pass.
  - **The window,** for 25 s on a copy of the M4 template world: the camera stayed at the player's
    eyes, the server had the feet 1.62 below them, and the time of day went up 20 a second.
  - **A 4-second bench** in a dev build flew as before, and chunks streamed in during the flight
    (329, then 343).
- **Tests:**
  - the client's tests went from 97 to 113: the clock 4, the player 5, and `Game::apply` 7, one for
    each packet's effect;
  - clippy is clean.

### Step 6: walking, flying and the honest input (2026-10-27)

- **Commits:** `895c7c5` "Read blocks' collision boxes from pumpkin-data", and `3dd914f` "Walk,
  swim, climb and fly under physics matched to the official client" (Deviations 17 to 20).
- **`player/physics.rs`:** one tick as a pure function over a block lookup.
  - **The rules:** the keys' push, then the move, y first and then the larger of x and z, stepping
    up 0.6. Sneaking stops at edges. Then friction, drag and gravity, with water, lava and ladders.
  - **Bedrock's own rules** are in Deviation 17.
- **`player/movement.rs`:**
  - **The modes:** a double tap on jump toggles flight in creative, and a spectator always flies
    through blocks.
  - **Sprinting and sneaking** start and stop with the keys.
  - **Each tick's input:** the flags, `move_vec` and `raw_move`, set from what the keys and the
    physics did. The velocity is `delta`.
- **`player/terrain.rs`:** the decoded world as physics sees it.
  - It reads blocks through M3's table and pumpkin-data, and a waterlogged block's water from
    layer 1.
  - The player waits until the 3 × 3 chunks around it have arrived.
- **The official client's traces,** all 12, replayed tick by tick (`player/traces.rs`):

  | Run | Ticks | Worst position | Worst `delta` | Ticks that differ |
  | --- | --- | --- | --- | --- |
  | walk | 167 | 0.000015 | 0 | 0 |
  | sprint | 129 | 0.000015 | 0 | 0 |
  | sneak | 137 | 0.000015 | 0 | 0 |
  | jumps | 68 | 0.000015 | 0 | 0 |
  | walk_turned | 83 | 0.000015 | 0 | 0 |
  | walk_diagonal | 110 | 0.000015 | 0 | 0 |
  | sprint_jump | 115 | 0.000015 | 0 | 0 |
  | fall | 39 | 0 | 0 | 0 |
  | water | 131 | 0 | 0 | 0 |
  | water_exit | 67 | 0.000001 | 0 | 0 |
  | ladder | 100 | 0 | 0 | 0 |
  | fly | 110 | 0 | 0 | 0 |

  - **What "differ" checks:** a tick differs if its position or `delta` is more than 0.001 off, or
    if any flag, `move_vec` or `raw_move` differs.
  - **The 0.000015:** one `f32` step at y 201, on the tick after each teleport. The official
    client already rests a step higher there.
- **The window:**
  - **Movement:** the keys move the player under physics.
  - **A 20-second run** on a copy of the M4 template world: the player stood at feet 74.00, with
    the camera at its eyes and the server's copy where the client put it.
  - **Walking around in a window** is the checkpoint's to try.
- **Headless:** all seven checks pass.
  - **The dev world's host was underground** before Step 6's first run, inside the stone at
    (-721, -8, -27). Something with M5's earlier flight, which passed through terrain, had left it
    there. The teleport check lifted it 5 blocks, and physics brought it down onto the next ledge.
- **Tests:**
  - the client's tests went from 113 to 132: physics 11, movement 4, the traces 1, the shapes 2,
    and the player 1;
  - clippy is clean.

### Step 7: Pumpkin's shapes for the stand-in cubes (2026-10-27)

- **Commit:** `9a6fdf1` "Draw Pumpkin's block shapes where M4 drew stand-in cubes".
- **`Shape::Boxes`:** a block the tables draw as a cube, whose Java state isn't one, is drawn as
  that state's outline boxes, rounded to sixteenths.
  - **Textures:** each face shows the block's texture for that face, cut by its position, as a
    slab's does.
  - **Culling:** a face on the block's edge is hidden by an opaque neighbour; one inside the block
    never is.
  - **What it covers:** 493 block names: stairs, fences, doors, ladders, torches, trapdoors,
    buttons, signs, rails and the rest.
- **One state for both:** `BlockTable::shape_state` is the Java state physics collides with and the
  mesher draws (Deviation 21).
- **The check of every Bedrock id** (owner, at approval): 184 ids have Java states with different
  boxes. All of them come from properties Java keeps in the block state and Bedrock keeps somewhere
  else, or works out itself:

  | Ids | Blocks | Why |
  | --- | --- | --- |
  | 60 | every button | Java turns a floor or ceiling button with `facing`; Bedrock's don't turn. Only the outline differs, a small box |
  | 49 | fire, redstone wire, chorus plants, vines | connections, which Bedrock works out from the blocks around |
  | 40 | every chest | Java's `type` pairs chests; Bedrock keeps the pairing in the block actor. Shapes take a single chest |
  | 12 | pistons, sticky pistons | Java's `extended`: Bedrock keeps the push in the block actor. Shapes take the piston drawn back |
  | 12 | piston heads | Java's `short` arm, a frame of a piston's push |
  | 8 | scaffolding | Java's `bottom` only adds to the outline; the collision is the same |
  | 3 | kelp, weeping and twisting vines | Java's head and body are two blocks, Bedrock's one. Drawn as crossed planes, with no collision |

  They're in the test with these reasons, for the owner to confirm (Deviation 22).
- **The shapes, through the Bedrock ids:**
  - a stair is two boxes, and an inner corner three;
  - an east-connected fence is one box, from its post to the east edge, since Pumpkin's shapes
    come merged;
  - an open door and a closed one are each a box 3 sixteenths thick, in different places;
  - a torch is one box (6, 0, 6 to 10, 10, 10), and a ladder a plate on its wall;
  - a piston is drawn back.
- **The window,** on a copy of the M4 template world:
  - the join meshed as before (1,269,618 quads);
  - the census of approximate shapes counted pointed dripstone 11,516, leaf litter 2,082, chests
    15 and wildflowers 3.
- **Checks:**
  - **Headless:** all seven checks pass.
  - **Tests:** the client's went from 132 to 136. The real-install test, which reads the vanilla
    packs, passes too.
  - **Clippy** is clean.
- **Not tried yet:** walking the shapes in a window, over stairs and fences and through doors. That's
  the owner checkpoint.
- **The owner's walk (checkpoint 1, 2026-10-30):** the physics felt right. The special models were
  broken, which became Step 7b (Deviation 23). The owner hasn't yet confirmed the shape exceptions
  above.

### The walls that gave way (2026-11-01)

The owner, in a world whose spawn is at x -588, found they could walk through blocks at some yaws, wherever they
went, and logged the poses at x -535.88, z -10.13.

- **Commit:** `ad5e250`.
- **The cause is the axis and the distance, not the angle.** A player stopped by a wall has its box's edge exactly
  on the wall's face. Then its position rounds to the nearest `f32`, whose steps widen with distance from the origin.
  Half the player's width, 0.3, decides which way that goes, and it's the same walking either way along an axis:

  | Wall at, either side of the origin | An `f32`'s step | The box, after rounding | |
  | --- | --- | --- | --- |
  | 100 | 0.000008 | 0.000003 short of the face | Held |
  | 200 | 0.000015 | 0.000003 short of the face | Held |
  | 300 | 0.000031 | 0.000012 past the face | Gave way |
  | 535 | 0.000061 | 0.000012 past the face | Gave way |
  | 2000 | 0.000122 | 0.000049 short of the face | Held |
  | 5000 | 0.000488 | 0.000195 past the face | Gave way |
  | 20000 | 0.001953 | 0.000781 short of the face | Held |

  Collisions forgive 0.00001. Past that, the box counts as inside the block, and a block doesn't stop a box that's
  inside it, so the player walks on. Each block ahead rounds the same way, so it walks on through them all. At the
  owner's spot, every wall along x gave way and none along z, whose coordinate was near 0. So it looked like yaw.
- **At the owner's spot,** in a copy of its world, with a temporary check that's since been removed. Each run walks
  forward for 100 ticks from `-535.88 -42.00 -10.13`, on the real server and the real chunks:

  | Yaw | Ended, before | Ticks inside blocks, before | Ended, after | Ticks inside blocks, after |
  | --- | --- | --- | --- | --- |
  | -76.1 | x -518.69 | 77 | x -531.30, at the wall | 0 |
  | -68.7 | x -520.33 | 77 | x -531.30, at the wall | 0 |
  | 85.8 | x -556.72, down a slope | 0 | The same | 0 |
  | 86.7 | x -556.74, down a slope | 0 | The same | 0 |
  | 0 | z -7.30, at the wall | 0 | The same | 0 |
  | 180 | z -24.70, at the wall | 0 | The same | 0 |

  The first tick inside had the box 0.0000122 past the face, as the table above says it would. Yaws 85.8 and 86.7 met
  no wall from there, so these runs don't show that direction; the tests below do.
- **Why nothing caught it:** every trace was recorded within 40 blocks of the origin on x and z, and every test's
  wall stood within a few blocks of it.
- **The new tests** walk at walls, on every side, from x 0 to the edge of the world at 29,999,984:
  - before the fix, 92 of the 168 kinds of wall in `a_wall_stops_the_player_however_far_it_is_from_the_origin` let
    the player through, by up to 58 blocks in the first version's longer walk;
  - a move that ends just short of a wall, with nothing to stop it, could round in as well: 506 of 924 got through
    before the fix, and none now;
  - after it, a player still slides along a wall, fits through a one-block gap, and walks out of a block it's inside;
  - `every_wall_stops_the_player`, ignored for its 111 s in a release build, takes every yaw in 10-degree steps, every
    gait, and faces on every sixteenth, for fences and doors: 731,136 walks, all stopped.
  - Ceilings and floors held at every height from -64 to 320, before the fix and after. Their test stays.
- **Checks:** 293 client tests, the official client's traces among them, and clippy with the same 2 warnings.
- **Not verified:** nobody has walked at a wall in the window with this in.
- **Left:** a player saved against a wall by an older build starts a hair inside it, so that one wall gives way
  once.

### Step 9: Items, icons and the hotbar (2026-11-01)

- **Commits:** `e02a1a2` (the items and their icons) and `ceae086` (the keys).
- **The census:** of 2,076 items known, the server can send 1,587. 538 have a picture, 998 draw as blocks, in
  4,070 quads, and 51 draw the missing texture: spawn eggs for newer mobs and harnesses, whose files aren't
  among the others, and a few that are never held, such as air.
- **At startup,** in a release build: 586 sprites in a 1024 × 691 atlas, 157 ms for the whole UI. `PackStack::list`
  is what keeps that down: it says which pack has each file, so about a thousand pictures don't each ask all 55
  packs in turn. Icons are meshed in parallel.
- **The bench** first read 0.87 ms against Step 8's 0.43, which nothing in this step could explain. So the old
  build and the new took turns, 15 s each, at radius 8:

  | | Round 1 | Round 2 | Round 3 |
  | --- | --- | --- | --- |
  | Before the HUD | 0.80 ms | 0.88 ms | 0.72 ms |
  | Step 9 | 0.82 ms | 0.85 ms | 0.75 ms |

  It was the machine, with other programs busy, and a frame costs about what it did. The two runs below it in
  Step 8's table are from a quieter machine, so neither table can be set against the other.
- **Against the real server,** with a temporary check that's since been reverted:
  - a hotbar filled on the server reached the client as packets, with the same counts on both sides;
  - Q took 5 dirt to 4 on both sides, once, and dropping a slot's last item emptied it;
  - selecting slot 2 moved the server's selected slot to 2.
- **Byte for byte:** Q's transaction matches the official client's. The first try didn't: it needed a falling
  even id and a line naming the slot it predicts (container 29), which Results, Step 1 had written down.
- **Checks:** 288 client tests, the headless run's 7 PASS lines, and clippy with the same 2 warnings.
- **Not verified:** nobody has pressed 1 to 9, the wheel or Q in the window, since the checks call what they call.
  Nobody has seen the icons beside the official client: their angle and shading are Java's GUI numbers.
- **Left:**
  - all potions show one bottle, and an item whose `item_texture.json` key has several pictures shows its first;
  - no durability bars, no enchantment glint;
  - a name stays for 3 seconds, then goes, with no fade;
  - blocks with no item picture show their world model, so a few differ from Bedrock's icons.

### Step 8: The UI, and the HUD (2026-11-01)

- **Commits:** `1fe5995` (this plan's lost heading), `2daa7a4` (breath), `3c8173c` (sprites, text, strings
  and layout) and `699647b` (drawing, and the HUD).
- **At startup:** 29 sprites in a 1024 × 515 atlas, none missing, with ASCII and 4 glyph pages, which are what
  `en_US.lang` needs; 13,413 strings, with all 10 that the menus will want; 122 ms in a release build.
- **Where things go,** from the install's `ui/hud_screen.json`, read while writing this and not at runtime:
  - **The hotbar's bottom is 1.5 units above the window's.** Its panel is 31 tall, 13 up, and the hotbar
    hangs 16 below the middle of the 5-tall experience bar at the top of it. So a selected slot's frame
    has room for its 24 units, as it wouldn't if the hotbar were flush.
  - **Hearts and hunger are 41 units up,** 8 apart, spanning 91 units either side of the middle. That the
    renderer draws from its control's top is inferred: it fits a 1-unit gap above the experience bar.
- **The bench,** at radius 8 in creative, one after the other on the same day:

  | | Average | p50 | p95 | p99 | fps |
  | --- | --- | --- | --- | --- | --- |
  | Before | 0.41 ms | 0.35 | 0.72 | 1.24 | 2,430 |
  | With the HUD | 0.43 ms | 0.36 | 0.76 | 1.17 | 2,337 |

  The HUD stays on in the bench, since it's part of a real frame.
- **In a real window:** `--debug-world` and the bench ran clean, with the surface in `Bgra8UnormSrgb` and the
  UI in `Bgra8Unorm`, and the block's name above the hotbar followed typed jumps.
- **Found by the tests:** a full breath showed an eleventh, bursting bubble. A tenth of a bubble per tick
  isn't exact in an `f32`, so the sum now multiplies first.
- **Found by looking:** text was garbled until `default8.json` turned up (Deviation 34).
- **Checks:** 260 client tests, the headless run's 7 PASS lines, and clippy with the same 2 warnings as
  before. `hud_sheets` draws every piece over stripes, and `screenshots` takes real frames.
- **Not verified:** nobody has seen the HUD beside the official client. Its height above the bottom, and
  whether its crosshair inverts, are the things to look at.
- **Left:**
  - hearts don't flash, wobble at low health, or change for poison, wither, frost or absorption;
  - more than ten hearts stay ten;
  - the experience bar isn't in Decision 15, so its gap stays empty;
  - text is white for now, as colours arrive with chat in Step 16;
  - nothing updates the selected slot or the armour row until Step 9.

### Step 7d: Animated textures, fluids that flow, and a debug world (2026-10-31)

- **One code commit,** not one per concern as usual: the session ended before it could be split.
- **Animation:** 46 layers move, with 650 frames, 3.5 MB. The bench's average frame stays 0.32 ms, and
  its p99 goes from about 0.70 ms to 0.76 ms.
- **The irregular lava,** seen on a sheet first: each block turned its texture at random, as `isotropic`
  says for a cube, and a flow was flat steps with gaps. Now a pool tiles evenly, and a flow slopes from
  block to block with its texture running downhill.
- **The debug world:** 15,958 places to go, 16,402 blocks in 265 chunks, written in about 150 ms, with
  no change to the fork. Typed searches and `--goto` went to the right place, and a jump held still, with
  the camera and the server's player agreeing.
- **Checks:** 191 client tests, headless not re-run on this commit.
- **Not verified:** nobody has seen the moving textures or the debug world in the window yet, or beside
  the official client. Flow follows Java's rules, so Bedrock may differ.
- **Left:** a corner of liquid beside a chunk that isn't loaded is a little off until its next mesh, a
  water surface can't be seen from below, and the debug world's time stays at sunrise.

### Step 7c: Block models from the user's own Java jar (2026-10-31)

- **Commits:**
  - `b1e4e85` "Give vertices 1,024ths of a block and 128ths of a texture";
  - `4301c30` "Stand for a Bedrock id with the Java state nearest its default";
  - `c7e6e5b` "Find the user's Java jar, and compile block models from it";
  - `ac2f3c5` "Load textures from the jar, and give one picture one layer";
  - `dd87269` "Draw the jar's models, in Bedrock's textures where they fit";
  - `ee3076e` "Add a script that compares compiled models with Blockbench's";
  - `3585e05` "Choose Bedrock's art by name where it's nearly the same picture".
- **The law:** PLAN.md, "Resource packs", has the reading and the rules. In short, a tighter encoding
  of Mojang's models would still be a copy, so the client reads the user's own jar at runtime, as the
  plugin does, and keeps the result in memory only.
- **The encoding,** for the 15,953 states Bedrock has, on 26.3:

  | | |
  | --- | --- |
  | Read from the jar | 3,850 files, 2.3 MB of JSON |
  | Every model baked on its own | 73,029 quads |
  | Kept, sharing quads between blocks that differ only by texture | 16,826 quads in 1,201 shapes, 606 KB |
  | Time | 108 ms in a release build, at every startup |

  At 108 ms a cache on disk isn't worth having, so nothing derived from the jar is ever written.
- **The window,** on one copy of the M4 template world, in a release build:

  | | With the jar | Without |
  | --- | --- | --- |
  | Everything before the server starts | 595 ms | 411 ms |
  | Average frame | 0.33 ms | 0.32 ms |
  | Faces on the GPU at the end | 1,508,927 | 1,522,132 |
  | Texture array | 1,182 layers, 6.5 MB | 1,133 layers, 6.2 MB |
  | Block names drawn as Pumpkin's boxes | 64 | 288 |

  Without a jar, the contact sheets are the same as Step 7b's to the byte.
- **The double check against the Asset Browser's program:** `scripts/compare-models-with-blockbench.mjs`
  starts Blockbench with a throwaway profile and has it load every model with the call the plugin
  makes, then compares its meshes with what we bake.
  - **Of 2,519 models, all 24,714 quads agree:** corners, texture coordinates, cull faces and tint
    indexes, with no face over on either side. That covers tilted and rescaled boxes, and the two with
    rotations about several axes.
  - **What it can't check:** a block state's turn and `uvlock`, since Blockbench opens one model at a
    time. The next check covers those.
  - **A picture of 37 models from each,** with the same camera, was how the locked repeaters showed
    (Deviation 27). The numbers couldn't see that, as the wrong state's model was built correctly.
- **Every Java state against a real jar** (ignored tests, with `LIMONITE_JAVA_JAR`):
  - **All 35,723 compile,** with no missing file, no state needing more than 7 textures, and every
    texture in the jar.
  - **Doors, trapdoors, stairs, slabs, buttons, pressure plates, fences, panes, bars and shelves** lie
    within half a sixteenth of Pumpkin's outlines in every state, which only holds if they're turned
    the right way. The other 335 blocks that differ are printed; the differences are of the expected
    kinds, such as plants and flames sticking out of small hitboxes, and open gates.
  - **83 blocks have states that share a Bedrock id and look different.** A new one fails the test:

    | Blocks | Why |
    | --- | --- |
    | Beds, and flower pots with something in them | Bedrock keeps the colour and the plant in the block actor |
    | Pistons, sticky pistons and piston heads | The same, for whether it's pushed out |
    | Redstone wire, chorus plants, fire, vines, repeaters, scaffolding | Bedrock works out their connections, `locked` and bottoms from the blocks around |
    | Buttons and levers | Java turns them on the floor and ceiling, and Bedrock doesn't |
    | Kelp and the three vines | Java's head and body are two blocks, and Bedrock's one |

- **Textures:** 347 of the ones models name are Bedrock's, and 331 Java's.
  - **Most of Java's have the same pixels in the install** under another name, so they share its layer:
    the array only grows by 49 layers.
  - **Java's own are what Bedrock lays out differently:** signs and beds, which it only has as entity
    atlases; pane edges, chains and redstone dust; the bamboo gate; and dried ghasts.
- **What's left:**
  - **No model in the jar, so boxes until M8:** chests, shulker boxes, banners, heads, decorated pots,
    conduits and copper golem statues.
  - **Worked out from the blocks around, and not done:** redstone wire draws as a dot, chorus plants as
    a stem, repeaters never lock, and fire only burns on the floor.
  - **Weighted models show their first:** Java picks by position, and Bedrock's way isn't known.
  - **Not yet beside the official client.** The jar is Java's, so where Bedrock's engine draws a block
    differently, this will too. Lit redstone torches are the first to check: they keep Java's glow boxes.

### Step 7b: Bedrock's textures and shapes for the blocks Step 7 got wrong (2026-10-30)

- **Commits:**
  - `a5fc3b8` "Draw block states offscreen into contact sheets", then `bc0e9a6`;
  - `98a8c54` "Add the dry foliage tint, and tints of one fixed colour";
  - `0650fe8` "Let a block's vertices reach outside it";
  - `bf19039` "Pick each block's textures and shape by its state";
  - `10a0f25` "Show plants' tips and snowy sides by the block next to them".
- **The evidence, before any fix:** the contact sheets (`render/sheet.rs`), 14 PNGs drawn through the
  real mesher, texture array and shader. They showed the owner's four and much more:
  - dripstone was a column with a different growth stage on each side, always the hanging ones;
  - a door's lower half showed its upper half, and spruce, birch, jungle, acacia, dark oak and iron
    doors were all oak;
  - every chest and furnace faced south, and an uncarved pumpkin had a face;
  - leaf litter was white: its texture is grey, and takes the dry-foliage colormap;
  - a fence was a solid wall, since its outline is; straight rails showed a curve; chains and candles
    were invisible, since a box showed the see-through part of their textures; a torch was a hollow
    tube of four sticks.
- **Why** (Deviation 23): `blocks.json`'s slots aren't always faces, and Pumpkin's outlines are for
  aiming. There's no geometry for vanilla blocks in the install, and no chest, bed or sign model
  either: the packs' model archives hold other entities, and those aren't among them.
- **Part 1, the textures:** `blocks/rules.rs` says which key and variant each slot shows.
  - **Over every name Pumpkin can send,** 345 have a choice of textures, and the real-install test
    accounts for each:

    | Names | Why they draw right |
    | --- | --- |
    | 212 | a rule in `rules.rs` |
    | 59 | their slots really are their faces |
    | 40 | turned by `pillar_axis` |
    | 14 | leaves: their variants are fancy then fast, and the fancy one draws |
    | 8 | their first variant is the right one |
    | 7 | `cross_key` picks these by state |
    | 1 | bamboo picks its own slots |
    | 2 | banners, stand-ins until M8 |
    | 2 | not done, on purpose: the pitcher crop, only grown from a sniffer's seed, and the jigsaw block, only placed by a command |

- **Part 2, the shapes:** `blocks/models.rs` has 60-odd hand-made models, written in the terms of
  Java's model files, because Bedrock's textures are laid out for the same shapes. Each is baked into
  flat quads at startup, once, so the mesher only copies them.
  - **Which way a model turns** comes from the Java state Pumpkin's table gives for the Bedrock id.
  - **The check:** a test walks every Java state of 30-odd families, and fails if a model leaves
    Pumpkin's outline for that state. It found the coral fans too long, and confirmed every door,
    gate, rail, torch, lantern, chain, rod, anvil, bell and campfire. The test can't see what's
    symmetrical in its outline, such as a repeater's direction: those follow Java's block states.
- **What's next door:** kelp and vines show their tip where the plant ends, and grass, mycelium and
  podzol show a snowy side under snow.
- **Tints:** leaf litter takes `dry_foliage.png`, lily pads and stems a fixed colour, redstone wire
  one by its power, and bush takes grass's. A census of the install's grey textures with no tint
  found only bush left.
- **The window,** on a copy of the M4 template world:
  - the join meshed 1,262,571 quads, down from 1,269,618;
  - 288 names are still drawn as Pumpkin's boxes, down from 493. For stairs, walls, buttons,
    pressure plates and trapdoors, that is their model;
  - the census counted 15 chests, where Step 7's counted 11,516 pointed dripstone and 2,082 leaf
    litter.
- **Checks:**
  - **Tests:** 150 client tests pass, with the 3 that need the install or a GPU, up from 136.
  - **Headless:** all seven checks pass.
  - **Clippy** is clean.
- **Not checked against the official client:** the owner's walk is next. The models are written from
  Java's, so these are the likeliest to differ on Bedrock:
  - leaf litter's and petals' quarters, which way they fill in, and the petals' stems;
  - the tilt of a wall torch and a lever, and the top of a lectern, each rounded to the grid;
  - dripleaf leaves, as they tilt;
  - which way a trapdoor's texture turns, and glazed terracotta's.
- **Left to do:** STATUS.md's "Special shapes to do" has the list.
