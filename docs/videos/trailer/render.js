// Render terminal frames (HTML from cells.py) to PNG at 2x, one browser.
const { chromium } = require('/opt/node22/lib/node_modules/playwright');
const fs = require('fs');
(async () => {
  const dir = process.argv[2];
  const files = fs.readdirSync(dir).filter(f => f.endsWith('.html') && !fs.existsSync(dir + '/' + f.replace('.html', '.png')));
  const b = await chromium.launch({ executablePath: '/opt/pw-browsers/chromium', args: ['--no-sandbox'] });
  const p = await b.newPage({ viewport: { width: 1200, height: 748 }, deviceScaleFactor: 2 });
  for (const f of files) {
    await p.goto('file://' + dir + '/' + f);
    await p.evaluate(() => document.fonts.ready);
    await p.screenshot({ path: dir + '/' + f.replace('.html', '.png'), clip: { x: 0, y: 0, width: 1200, height: 748 } });
  }
  await b.close();
  console.log('rendered', files.length);
})();
