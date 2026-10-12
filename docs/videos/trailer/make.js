// Render trailer.html frame by frame into an MP4 (or stills with --stills t1,t2…).
const { chromium } = require('/opt/node22/lib/node_modules/playwright');
const fs = require('fs'), { spawn } = require('child_process');
const dir = __dirname;
(async () => {
  const html = fs.readFileSync(dir + '/trailer.html', 'utf8')
    .replace('__SCENES__', fs.readFileSync(dir + '/scenes.json', 'utf8'));
  fs.writeFileSync(dir + '/built.html', html);
  const b = await chromium.launch({ executablePath: '/opt/pw-browsers/chromium', args: ['--no-sandbox'] });
  const p = await b.newPage({ viewport: { width: 1920, height: 1080 } });
  await p.goto('file://' + dir + '/built.html');
  await p.evaluate(() => Promise.all([document.fonts.ready, ...[...document.images].map(i => i.complete ? 0 : new Promise(r => { i.onload = r; i.onerror = () => r(console.log('bad ' + i.src)); }))]));
  p.on('console', m => console.log(m.text()));
  const total = await p.evaluate(() => window.TOTAL);
  const stills = process.argv.indexOf('--stills');
  if (stills > 0) {
    for (const t of process.argv[stills + 1].split(',')) {
      await p.evaluate(t => render(t), +t);
      await p.screenshot({ path: `${dir}/still-${t}.png` });
    }
    await b.close(); return;
  }
  const fps = 30, n = Math.ceil(total / 1000 * fps);
  const ff = spawn('ffmpeg', ['-y', '-f', 'image2pipe', '-framerate', String(fps), '-c:v', 'mjpeg', '-i', '-',
    '-c:v', 'libx264', '-preset', 'slow', '-crf', '18', '-pix_fmt', 'yuv420p', '-movflags', '+faststart',
    dir + '/tasq-trailer.mp4'], { stdio: ['pipe', 'ignore', 'inherit'] });
  for (let i = 0; i < n; i++) {
    await p.evaluate(t => render(t), i * 1000 / fps);
    const buf = await p.screenshot({ type: 'jpeg', quality: 95 });
    if (!ff.stdin.write(buf)) await new Promise(r => ff.stdin.once('drain', r));
  }
  ff.stdin.end();
  await new Promise(r => ff.on('close', r));
  await b.close();
  console.log('frames', n, 'ms', total);
})();
