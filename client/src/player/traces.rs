//! The official client's physics traces (m5-plan Decision 5), replayed.
//!
//! A trace is the `PlayerAuthInput`s the official client sent during one
//! clean run, a packet a tick: the keys it read, its angles, and where its
//! own physics put it (`fixtures/README.md`). From each trace's first tick,
//! every later tick's keys and yaw go through [`Movement::tick`] over the
//! world the run was recorded in, and what comes out must match what the
//! official client sent: the position and `delta` within 0.001 blocks,
//! `move_vec` and `raw_move`, and every flag.

use std::fmt::Write as _;

use glam::{DVec2, DVec3};
use pumpkin_protocol::{
    bedrock::server::player_auth_input::{InputData, SPlayerAuthInput},
    serial::PacketRead,
};
use pumpkin_util::GameMode;

use super::{
    EYE_HEIGHT,
    movement::{Flags, Keys, Movement, Situation},
    physics::{
        Body, Outcome,
        tests::{Kind, World},
    },
};

/// The most a replayed position or `delta` may differ from the official
/// client's, in blocks.
const TOLERANCE: f64 = 0.001;

/// The world the traces were recorded in, from the fixtures' README.
fn recorded(x: i32, y: i32, z: i32) -> Kind {
    // The ladder column, on the strip's last row, with three ladders on its
    // north face.
    if x == 0 && z == 40 && (200..=205).contains(&y) {
        return Kind::Stone;
    }
    if x == 0 && z == 39 && (200..=202).contains(&y) {
        return Kind::LadderNorth;
    }
    // The strip, its top at y 200.
    if y == 199 && (-8..=8).contains(&x) && (0..=40).contains(&z) {
        return Kind::Stone;
    }
    // The basin: glass, holding water 3 deep.
    if (12..=18).contains(&x) && (200..=203).contains(&y) && (0..=6).contains(&z) {
        let inside = (13..=17).contains(&x) && (201..=203).contains(&y) && (1..=5).contains(&z);
        return if inside { Kind::Water } else { Kind::Stone };
    }
    Kind::Air
}

/// The packets in a trace: each a little-endian `u32` length, then a body.
fn read(mut bytes: &[u8]) -> Vec<SPlayerAuthInput> {
    let mut ticks = Vec::new();
    while !bytes.is_empty() {
        let (length, rest) = bytes.split_at(4);
        let length = u32::from_le_bytes(length.try_into().unwrap()) as usize;
        let (mut body, rest) = rest.split_at(length);
        ticks.push(SPlayerAuthInput::read(&mut body).unwrap());
        bytes = rest;
    }
    ticks
}

fn has(tick: &SPlayerAuthInput, flag: InputData) -> bool {
    tick.input_data.get(flag as usize)
}

/// The keys the official client read on a tick, from its raw key flags.
fn keys(tick: &SPlayerAuthInput) -> Keys {
    Keys {
        forward: has(tick, InputData::Up),
        back: has(tick, InputData::Down),
        left: has(tick, InputData::Left),
        right: has(tick, InputData::Right),
        jump: has(tick, InputData::JumpCurrentRaw),
        sneak: has(tick, InputData::SneakCurrentRaw),
        sprint: has(tick, InputData::SprintDown),
    }
}

fn eyes(tick: &SPlayerAuthInput) -> DVec3 {
    let p = tick.position;
    DVec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z))
}

fn delta(tick: &SPlayerAuthInput) -> DVec3 {
    let d = tick.delta;
    DVec3::new(f64::from(d.x), f64::from(d.y), f64::from(d.z))
}

fn flag_numbers(flags: Flags) -> String {
    (0..66)
        .filter(|&bit| flags.0 & (1 << bit) != 0)
        .map(|bit: u32| bit.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// How a replay went.
#[derive(Debug, Default)]
struct Replay {
    ticks: usize,
    worst_position: f64,
    worst_delta: f64,
    /// One line for each tick that didn't match, by tick.
    mismatches: Vec<String>,
}

fn replay(bytes: &[u8], gamemode: GameMode) -> Replay {
    let ticks = read(bytes);
    let first = &ticks[0];
    let mut body = Body {
        feet: eyes(first) - DVec3::Y * EYE_HEIGHT,
        velocity: delta(first),
        on_ground: has(first, InputData::VerticalCollision) && first.delta.y < 0.0,
    };
    let mut movement = Movement {
        sprinting: has(first, InputData::Sprinting),
        sneaking: has(first, InputData::Sneaking),
        last: keys(first),
        last_outcome: Outcome {
            horizontal_collision: has(first, InputData::HorizontalCollision),
            vertical_collision: has(first, InputData::VerticalCollision),
            jumped: false,
        },
        ..Movement::default()
    };
    let situation = Situation {
        gamemode,
        may_fly: gamemode == GameMode::Creative,
        hunger: 20.0,
        movement: 0.1,
        gravity: 0.08,
        fly_speed: 0.05,
    };
    let world = World(recorded);
    let mut replay = Replay {
        ticks: ticks.len(),
        ..Replay::default()
    };
    for (i, tick) in ticks.iter().enumerate().skip(1) {
        let report = movement.tick(&mut body, keys(tick), tick.yaw, &situation, &world, false);
        let mut wrong = String::new();
        let position = (body.feet + DVec3::Y * EYE_HEIGHT - eyes(tick)).abs().max_element();
        let velocity = (body.velocity - delta(tick)).abs().max_element();
        replay.worst_position = replay.worst_position.max(position);
        replay.worst_delta = replay.worst_delta.max(velocity);
        if position > TOLERANCE {
            let _ = write!(wrong, " position off by {position:.6} (ours {}, theirs {});", body.feet + DVec3::Y * EYE_HEIGHT, eyes(tick));
        }
        if velocity > TOLERANCE {
            let _ = write!(wrong, " delta off by {velocity:.6} (ours {}, theirs {});", body.velocity, delta(tick));
        }
        let theirs = Flags(tick.input_data.bits);
        if report.flags != theirs {
            let _ = write!(
                wrong,
                " flags [{}], theirs [{}];",
                flag_numbers(report.flags),
                flag_numbers(theirs)
            );
        }
        for (name, ours, theirs) in [
            ("move_vec", report.move_vec, tick.move_vec),
            ("raw_move", report.raw_move, tick.raw_move),
        ] {
            let theirs = DVec2::new(f64::from(theirs.x), f64::from(theirs.y));
            if (ours - theirs).abs().max_element() > 1e-4 {
                let _ = write!(wrong, " {name} {ours}, theirs {theirs};");
            }
        }
        if !wrong.is_empty() {
            replay.mismatches.push(format!("tick {i}:{wrong}"));
        }
    }
    replay
}

#[test]
fn the_official_clients_runs_replay_within_a_thousandth_of_a_block() {
    let runs: [(&str, &[u8], GameMode); 12] = [
        ("walk", include_bytes!("fixtures/walk.bin"), GameMode::Survival),
        ("sprint", include_bytes!("fixtures/sprint.bin"), GameMode::Survival),
        ("sneak", include_bytes!("fixtures/sneak.bin"), GameMode::Survival),
        ("jumps", include_bytes!("fixtures/jumps.bin"), GameMode::Survival),
        ("walk_turned", include_bytes!("fixtures/walk_turned.bin"), GameMode::Survival),
        ("walk_diagonal", include_bytes!("fixtures/walk_diagonal.bin"), GameMode::Survival),
        ("sprint_jump", include_bytes!("fixtures/sprint_jump.bin"), GameMode::Survival),
        ("fall", include_bytes!("fixtures/fall.bin"), GameMode::Survival),
        ("water", include_bytes!("fixtures/water.bin"), GameMode::Survival),
        ("water_exit", include_bytes!("fixtures/water_exit.bin"), GameMode::Survival),
        ("ladder", include_bytes!("fixtures/ladder.bin"), GameMode::Survival),
        ("fly", include_bytes!("fixtures/fly.bin"), GameMode::Creative),
    ];
    let mut report = String::from("run            ticks  worst position  worst delta  mismatched ticks\n");
    let mut failed = Vec::new();
    for (name, bytes, gamemode) in runs {
        let replay = replay(bytes, gamemode);
        let _ = writeln!(
            report,
            "{name:<14} {:>5}  {:>14.6}  {:>11.6}  {}",
            replay.ticks,
            replay.worst_position,
            replay.worst_delta,
            replay.mismatches.len()
        );
        for line in replay.mismatches.iter().take(8) {
            let _ = writeln!(report, "    {line}");
        }
        if !replay.mismatches.is_empty() {
            failed.push(name);
        }
    }
    eprintln!("{report}");
    assert!(failed.is_empty(), "these runs don't match the official client: {failed:?}\n{report}");
}
