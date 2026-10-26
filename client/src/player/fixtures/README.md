# Physics traces from the official client

The ground truth for the client's physics (m5-plan Decision 5). Each trace is one clean run of the
official Bedrock client, as the `PlayerAuthInput` packets it sent: one per tick, in order. Each
packet carries the inputs the client read that tick and the position its own physics produced, so
a trace says both what was pressed and where it led. The physics tests (m5-plan Step 6) replay each
trace's inputs from its first tick and compare positions tick by tick.

- **Client:** official Bedrock 1.26.52 (Windows), protocol 2193, Fancy graphics, keyboard and
  mouse.
- **Server:** the Pumpkin fork at `443b954`, offline mode, over `NetherNet` on `127.0.0.1`, with a
  temporary capture hook in `BedrockClient::process_batch` that was never committed.
- **Captured:** 2026-10-26, in a world with seed `1790672995490159855`. Every run is in survival
  except `fly.bin`, which is in creative.
- **No identity:** these packets carry inputs, positions and ticks only.

## Format

Each `.bin` file is a sequence of packets. Each packet is a little-endian `u32` length, then the
packet body as the client sent it: after decompression, with no batch length or packet header.
`pumpkin_protocol::bedrock::server::player_auth_input::SPlayerAuthInput::read` reads a body.

- **The start:** at the run's teleport (flag 37, `HandledTeleport`), or with the player at rest.
- **The end:** 20 ticks after the last tick whose position changed.
- **Reading a tick:**
  - `position` is the eyes, 1.62 above the feet;
  - each tick's position is the previous tick's plus the previous tick's `delta`. So `delta` is
    the velocity carried into the next tick, after friction and drag;
  - the first tick after a teleport doesn't move.

## The world

Each run was set up with commands, so its blocks are known exactly:

- **The strip:** `/fill -8 199 0 8 199 40 stone`, a flat stone floor whose top is y 200, in open air.
  Standing on it, the eyes are at 201.62.
- **The water basin:** `/fill 12 200 0 18 203 6 glass`, then `/fill 13 201 1 17 203 5 water`. That
  makes water 3 deep over a glass floor, whose top is y 201, and inside glass walls up to y 204.
- **The ladder column:** `/fill 0 200 40 0 205 40 stone`, standing on the strip's last row. Ladders
  hang on its north face at (0, 200, 39), (0, 201, 39) and (0, 202, 39).

## The runs

| File | Ticks | What it shows | Numbers |
| --- | --- | --- | --- |
| `walk.bin` | 167 | `/tp @s 0.5 200 2.5 0 0`, then W for about 4 s, facing +z | 0.21586 blocks a tick at full speed (4.317 b/s) |
| `sprint.bin` | 129 | The same, with W and Ctrl | 0.28062 (5.612 b/s); `StartSprinting` once, then `SprintDown` and `Sprinting` each tick |
| `sneak.bin` | 137 | The same, with Shift held first, then W | 0.06476 (1.295 b/s); `StartSneaking` once, then `Sneaking`, `SneakDown` and `WantDown` each tick |
| `jumps.bin` | 68 | Three standing jumps; the second is a held Space, taken on the tick after landing | 0.42 on the first tick; peak 1.25224 after 6 ticks; airborne for 11 ticks |
| `walk_turned.bin` | 83 | `/tp @s 6.5 200 2.5 30 0`, then W for 3 s | The same speed as straight ahead, at yaw 30 |
| `walk_diagonal.bin` | 110 | `/tp @s 6.5 200 2.5 0 0`, then W and D for 3 s | `move_vec` is (-0.7071, 0.7071), and the speed is the same as straight ahead |
| `sprint_jump.bin` | 115 | `/tp @s 0.5 200 2.5 0 0`, then W, Ctrl and Space held for 3 s | Each take-off tick steps up to 0.61 blocks, as the sprint push adds to momentum from the last jump |
| `fall.bin` | 39 | `/tp @s 0.5 210 20.5 0 0`, then nothing until landed | 10 blocks in 18 ticks; `delta.y` follows `(v - 0.08) × 0.98` |
| `water.bin` | 131 | `/tp @s 15.5 202 3.5 0 0` into still water: sink, hold Space to rise, float, sink to the floor | Sinking settles at -0.025 a tick (`0.8 v - 0.005`); Space adds 0.04 a tick before the 0.8 drag |
| `water_exit.bin` | 67 | From rest on the basin floor: Space and W into the wall, out over it, and onto the glass rim | Two ticks of `delta.y` 0.3 with `HorizontalCollision`: the climb out of the water |
| `ladder.bin` | 100 | Walk to the ladders, climb them with W, stand on top, step back off, and fall past them | Climbs at 0.2 a tick with `HorizontalCollision`; falling past the ladders, the position drops by 0.2 a tick |
| `fly.bin` | 110 | In creative, on the strip: a jump, then a second tap of Space in the air to fly; rise, sink with Shift and land; rise again, then a double tap to stop, and fall | `StartFlying` (42) on the second tap. Landing doesn't end flight: Space still rises at flight speed (0.09, then 0.144), until a double tap sends `StopFlying` (43) |

**Flags by number** (`InputData`, in `pumpkin-protocol`):
- 3 `JumpDown`, 4 `SprintDown`, 6 `Jumping`, 8 `Sneaking`, 9 `SneakDown`;
- 10 `Up`, 12 `Left`, 13 `Right`, 16 `WantUp`, 17 `WantDown`, 20 `Sprinting`;
- 25 and 26 start and stop sprinting, 27 and 28 start and stop sneaking, 31 `StartJumping`,
  37 `HandledTeleport`, 42 and 43 start and stop flying;
- 48 `BlockBreakingDelayEnabled`, which is always set, 49 `HorizontalCollision`, 50
  `VerticalCollision`;
- 59 to 64, the raw jump and sneak key states.

The raw capture these came from is in `run/captures/m5-official/` (gitignored), and the fall was
recorded twice, identically.
