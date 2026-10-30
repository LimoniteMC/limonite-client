// The double check of m5-plan Step 7c: what Limonite bakes from a Java jar, against
// what Blockbench builds from the same files, face by face. Blockbench is the program
// behind the Asset Browser plugin, and has its own reader of Java's model format, so
// the two only agree if both read the format the same way.
//
// First write what Limonite bakes, and the model files, into a gitignored folder.
// They're derived from Mojang's files, so they must never be committed:
//
//   LIMONITE_JAVA_JAR=<the jar> LIMONITE_MODEL_DUMP=run/step7c/models \
//     cargo test -p limonite-client --bin limonite-client dump_every_model -- --ignored
//
// Then:
//
//   node scripts/compare-models-with-blockbench.mjs run/step7c/models
//
// This starts Blockbench with a throwaway profile inside that folder, so the user's
// own plugins, settings and recent files aren't touched, and talks to it over a local
// debug port. It closes that Blockbench again at the end. Set BLOCKBENCH if it isn't
// installed in the usual place. It prints counts and model names, and exits 1 if a
// quad differs.

import { spawn } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const PORT = 9333;
/// How far apart two corners can be, in sixteenths. Limonite rounds to a 64th.
const POSITION_SLACK = 0.03;
/// And two texture coordinates, in sixteenths. Limonite rounds to an eighth.
const UV_SLACK = 0.08;

const folder = process.argv[2];
if (!folder) {
  console.error('usage: node scripts/compare-models-with-blockbench.mjs <the dump folder>');
  process.exit(2);
}
const ours = JSON.parse(readFileSync(join(folder, 'limonite-models.json'), 'utf8'));
const modelFiles = readFileSync(join(folder, 'model-files.json'), 'utf8');
const names = Object.keys(ours).sort();

const profile = resolve(folder, 'blockbench-profile');
mkdirSync(profile, { recursive: true });
const executable = process.env.BLOCKBENCH ?? 'C:\\Program Files\\Blockbench\\Blockbench.exe';
const blockbench = spawn(executable, ['--userData', profile, `--remote-debugging-port=${PORT}`], { stdio: 'ignore' });

/// Blockbench's page, once it's up.
async function findPage() {
  for (let tries = 0; tries < 60; tries++) {
    await new Promise((done) => setTimeout(done, 1000));
    try {
      const targets = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
      const page = targets.find((target) => target.type === 'page');
      if (page) return page;
    } catch {
      // Not up yet.
    }
  }
  throw new Error('Blockbench never opened its debug port');
}

/// Runs in Blockbench: loads models as the plugin does, with a loader for their
/// parents, and returns each face's vertices and texture coordinates from the meshes
/// it would draw.
const buildInBlockbench = (batch) => `(async () => {
  const files = globalThis.__files;
  const loader = (path) => files[path];
  const inverse = new THREE.Matrix4();
  const out = {};
  const problems = [];
  for (const name of ${JSON.stringify(batch)}) {
    const text = files['models/' + name + '.json'];
    if (!text) { problems.push(name + ': no file'); continue; }
    try {
      newProject(Formats.java_block);
      Codecs.java_block.parse(JSON.parse(text), Date.now() + osfs + name.split('/').pop() + '.json', {
        resolve_parent: true,
        externalDataLoader: loader,
      });
      Canvas.updateAll();
      const root = Project.model_3d;
      root.updateMatrixWorld(true);
      inverse.copy(root.matrixWorld).invert();
      const faces = [];
      for (const cube of Cube.all) {
        cube.mesh.updateMatrixWorld(true);
        const position = cube.mesh.geometry.attributes.position.array;
        const uv = cube.mesh.geometry.attributes.uv.array;
        Canvas.face_order.forEach((direction, faceIndex) => {
          const face = cube.faces[direction];
          if (face.texture === null || face.texture === undefined) return;
          const verts = [];
          for (let i = 0; i < 4; i++) {
            const at = faceIndex * 4 + i;
            const v = new THREE.Vector3(position[at * 3], position[at * 3 + 1], position[at * 3 + 2]);
            v.applyMatrix4(cube.mesh.matrixWorld).applyMatrix4(inverse);
            verts.push([v.x, v.y, v.z, uv[at * 2] * 16, (1 - uv[at * 2 + 1]) * 16]);
          }
          faces.push({ direction, verts, cull: face.cullface || null, tint: face.tint });
        });
      }
      out[name] = faces;
      await Project.close(true);
    } catch (err) {
      problems.push(name + ': ' + err);
    }
  }
  return { out, problems };
})()`;

/// Whether a quad's corners are where a face's are, one for one. The face's index for
/// each corner if so.
function pairCorners(corners, verts) {
  const pairing = [];
  const used = new Set();
  for (const corner of corners) {
    const at = verts.findIndex(
      (vert, index) => !used.has(index) && [0, 1, 2].every((axis) => Math.abs(corner[axis] - vert[axis]) <= POSITION_SLACK),
    );
    if (at < 0) return null;
    pairing.push(at);
    used.add(at);
  }
  return pairing;
}

/// A face with no width can't be seen, and Limonite doesn't keep them: a flat plane's
/// edges. Blockbench gives flat planes a thousandth of a sixteenth of thickness, so
/// this asks for a hundredth, far thinner than any face that's meant.
function hasArea(verts) {
  const length = (from, to) => Math.hypot(...[0, 1, 2].map((axis) => verts[to][axis] - verts[from][axis]));
  return Math.min(length(0, 1), length(0, 2)) > 0.01;
}

function compare(theirs) {
  const counts = {
    'quads that match, corners and texture': 0,
    'quads in the right place with other texture coordinates': 0,
    'quads with no face there': 0,
    'faces with no quad': 0,
    'quads with another cull face': 0,
    'quads with another tint index': 0,
  };
  const lists = { misplaced: new Set(), texture: new Set(), missing: new Set(), cull: new Set(), tint: new Set() };
  for (const name of names) {
    const faces = (theirs[name] ?? []).filter((face) => hasArea(face.verts)).map((face) => ({ ...face, used: false }));
    for (const quad of ours[name]) {
      const uvOf = (corner) => quad.uvs[corner];
      // Two faces can lie in one place, so the one that agrees in everything comes
      // first, then the one with the same texture coordinates.
      let found = null;
      let score = -1;
      for (const face of faces) {
        if (face.used) continue;
        const pairing = pairCorners(quad.corners, face.verts);
        if (!pairing) continue;
        const agrees = pairing.every((at, corner) =>
          [0, 1].every((axis) => Math.abs(uvOf(corner)[axis] - face.verts[at][3 + axis]) <= UV_SLACK),
        );
        const sameCull = (quad.cull ?? null) === face.cull;
        const sameTint = (quad.tint ?? -1) === face.tint;
        const faceScore = Number(agrees) * 4 + Number(sameCull) * 2 + Number(sameTint);
        if (faceScore > score) {
          found = { face, agrees };
          score = faceScore;
        }
      }
      if (!found) {
        counts['quads with no face there']++;
        lists.misplaced.add(name);
        continue;
      }
      found.face.used = true;
      if (found.agrees) {
        counts['quads that match, corners and texture']++;
      } else {
        counts['quads in the right place with other texture coordinates']++;
        lists.texture.add(name);
      }
      if ((quad.cull ?? null) !== found.face.cull) {
        counts['quads with another cull face']++;
        lists.cull.add(name);
      }
      if ((quad.tint ?? -1) !== found.face.tint) {
        counts['quads with another tint index']++;
        lists.tint.add(name);
      }
    }
    for (const face of faces) {
      if (!face.used) {
        counts['faces with no quad']++;
        lists.missing.add(name);
      }
    }
  }
  return { counts, lists };
}

let failed = false;
try {
  const page = await findPage();
  // On a fresh profile its database takes a moment, and closing a project needs it.
  await new Promise((done) => setTimeout(done, 8000));
  const socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((done, fail) => {
    socket.onopen = done;
    socket.onerror = fail;
  });
  let nextId = 1;
  const waiting = new Map();
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    waiting.get(message.id)?.(message);
    waiting.delete(message.id);
  };
  const evaluate = (expression) =>
    new Promise((done, fail) => {
      const id = nextId++;
      waiting.set(id, (reply) => {
        if (reply.result?.exceptionDetails) fail(new Error(JSON.stringify(reply.result.exceptionDetails).slice(0, 1000)));
        else done(reply.result?.result?.value);
      });
      socket.send(
        JSON.stringify({
          id,
          method: 'Runtime.evaluate',
          params: { expression, awaitPromise: true, returnByValue: true, timeout: 300000 },
        }),
      );
    });

  // Each model it loads would otherwise leave a notice that its textures aren't in a
  // resource pack. This Blockbench's profile is thrown away afterwards.
  await evaluate(`globalThis.__files = ${modelFiles}; Blockbench.showMessageBox = () => {}; 0`);
  const theirs = {};
  const problems = [];
  const batchLength = 60;
  for (let start = 0; start < names.length; start += batchLength) {
    const result = await evaluate(buildInBlockbench(names.slice(start, start + batchLength)));
    Object.assign(theirs, result.out);
    problems.push(...result.problems);
    console.log(`${Math.min(start + batchLength, names.length)} of ${names.length} models`);
  }
  socket.close();
  writeFileSync(join(folder, 'blockbench-models.json'), JSON.stringify(theirs));

  const { counts, lists } = compare(theirs);
  console.log(`\nBlockbench built ${Object.keys(theirs).length} models, with ${problems.length} problems`, problems.slice(0, 10));
  for (const [what, count] of Object.entries(counts)) console.log(String(count).padStart(7), what);
  for (const [what, list] of Object.entries(lists)) {
    if (list.size) console.log(`${what}: ${list.size} models:`, [...list].sort().slice(0, 60).join(' '));
  }
  failed = Object.entries(counts).some(([what, count]) => count > 0 && what !== 'quads that match, corners and texture') || problems.length > 0;
} finally {
  blockbench.kill();
}
process.exit(failed ? 1 : 0);
