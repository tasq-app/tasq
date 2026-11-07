// Render trailer.html frame by frame into an MP4 (or stills with --stills t1,t2…),
// then score it: the page's cues → sfx.py → a WAV muxed in. `--audio` redoes
// only the sound, on the last silent render.
const { chromium } = require('playwright');
const fs = require('fs'), { spawn } = require('child_process');
const dir = __dirname;
const VERTICAL = process.argv.includes('--vertical');
const [W, H] = VERTICAL ? [1080, 1350] : [1920, 1080];
const OUT = dir + (VERTICAL ? '/tasq-trailer-vertical.mp4' : '/tasq-trailer.mp4');
const TAG = VERTICAL ? 'v' : 'h';
const SILENT = OUT.replace('.mp4', '.silent.mp4');
function run(cmd, args) {
  return new Promise((ok, no) => spawn(cmd, args, { stdio: 'inherit' }).on('close', c => c ? no(new Error(cmd + ' ' + c)) : ok()));
}
async function score(sfx, total) {
  fs.writeFileSync(`${dir}/sfx-${TAG}.json`, JSON.stringify({ total, cues: sfx }));
  await run('python3', [dir + '/sfx.py', `${dir}/sfx-${TAG}.json`, `${dir}/sfx-${TAG}.wav`]);
  await run('ffmpeg', ['-v', 'error', '-y', '-i', SILENT, '-i', `${dir}/sfx-${TAG}.wav`, '-map', '0:v', '-map', '1:a',
    '-c:v', 'copy', '-c:a', 'aac', '-b:a', '160k', '-movflags', '+faststart', '-shortest', OUT]);
}
(async () => {
  const html = fs.readFileSync(dir + '/trailer.html', 'utf8')
    .replace('__SCENES__', fs.readFileSync(dir + '/scenes.json', 'utf8'))
    .replace('__VERTICAL__', VERTICAL ? 'true' : 'false');
  fs.writeFileSync(dir + '/built.html', html);
  const b = await chromium.launch({ executablePath: '/opt/pw-browsers/chromium', args: ['--no-sandbox'] });
  const p = await b.newPage({ viewport: { width: W, height: H } });
  await p.goto('file://' + dir + '/built.html');
  await p.evaluate(() => Promise.all([document.fonts.ready, ...[...document.images].map(i => i.complete ? 0 : new Promise(r => { i.onload = r; i.onerror = () => r(console.log('bad ' + i.src)); }))]));
  p.on('console', m => console.log(m.text()));
  const total = await p.evaluate(() => window.TOTAL);
  const sfx = await p.evaluate(() => window.SFX);
  if (process.argv.includes('--audio')) { await b.close(); await score(sfx, total); return; }
  const stills = process.argv.indexOf('--stills');
  if (stills > 0) {
    for (const t of process.argv[stills + 1].split(',')) {
      await p.evaluate(t => render(t), +t);
      await p.screenshot({ path: `${dir}/still-${TAG}-${t}.png` });
    }
    await b.close(); return;
  }
  const fps = 30, n = Math.ceil(total / 1000 * fps);
  const ff = spawn('ffmpeg', ['-y', '-f', 'image2pipe', '-framerate', String(fps), '-c:v', 'mjpeg', '-i', '-',
    '-c:v', 'libx264', '-preset', 'slow', '-crf', '18', '-pix_fmt', 'yuv420p', '-movflags', '+faststart',
    SILENT], { stdio: ['pipe', 'ignore', 'inherit'] });
  for (let i = 0; i < n; i++) {
    await p.evaluate(t => render(t), i * 1000 / fps);
    const buf = await p.screenshot({ type: 'jpeg', quality: 95 });
    if (!ff.stdin.write(buf)) await new Promise(r => ff.stdin.once('drain', r));
  }
  ff.stdin.end();
  await new Promise(r => ff.on('close', r));
  await b.close();
  await score(sfx, total);
  console.log('frames', n, 'ms', total);
})();
