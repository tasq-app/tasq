# Render a tmux `capture-pane -e` dump as a Ghostty-like HTML cell grid:
# block/quadrant/powerline/box glyphs drawn exactly, Nerd icons 2 cells wide
# when followed by a space.
import re, sys, html, os
import unicodedata
src = open(sys.argv[1], encoding='utf-8').read()
crop = [int(x) for x in sys.argv[2].split(',')] if len(sys.argv) > 2 else None  # top,bottom,left,right rows/cols
D = os.path.dirname(os.path.abspath(__file__))
CW, CH, FS = 10, 22, 16
DEF_FG, DEF_BG = '#cad3f5', '#24273a'
def c256(n):
    base = ['#000000','#cd0000','#00cd00','#cdcd00','#0000ee','#cd00cd','#00cdcd','#e5e5e5','#7f7f7f','#ff0000','#00ff00','#ffff00','#5c5cff','#ff00ff','#00ffff','#ffffff']
    if n < 16: return base[n]
    if n < 232:
        n -= 16; l = lambda v: 0 if v == 0 else 55 + v*40
        return '#%02x%02x%02x' % (l(n//36), l(n//6%6), l(n%6))
    v = 8 + (n-232)*10; return '#%02x%02x%02x' % (v, v, v)
rows = []
fg = bg = None; bold = False
for line in src.split('\n'):
    cells = []
    pos = 0
    for m in re.finditer(r'\x1b\[([0-9;:]*)m|([^\x1b])', line):
        if m.group(2) is not None:
            ch = m.group(2)
            w = 2 if unicodedata.east_asian_width(ch) in 'WF' else 1
            cells.append((ch, fg, bg, bold))
            if w == 2: cells.append(('', fg, bg, bold))
            continue
        p = [int(x) if x else 0 for x in re.split('[;:]', m.group(1))] or [0]
        i = 0
        while i < len(p):
            v = p[i]
            if v == 0: fg = bg = None; bold = False
            elif v == 1: bold = True
            elif v == 22: bold = False
            elif v == 39: fg = None
            elif v == 49: bg = None
            elif 30 <= v <= 37: fg = c256(v-30)
            elif 90 <= v <= 97: fg = c256(v-90+8)
            elif 40 <= v <= 47: bg = c256(v-40)
            elif v in (38, 48) and i+1 < len(p):
                if p[i+1] == 2: col = '#%02x%02x%02x' % tuple(p[i+2:i+5]); i += 4
                else: col = c256(p[i+2]); i += 2
                if v == 38: fg = col
                else: bg = col
            i += 1
    rows.append(cells)
if crop:
    t, b, l, r = crop
    rows = [row[l:r] for row in rows[t:b]]
W = max(len(r) for r in rows)
out = []
BLK = {'▄': [(0,.5,1,.5)], '▀': [(0,0,1,.5)], '█': [(0,0,1,1)], '▐': [(.5,0,.5,1)], '▌': [(0,0,.5,1)],
       '▗': [(.5,.5,.5,.5)], '▖': [(0,.5,.5,.5)], '▝': [(.5,0,.5,.5)], '▘': [(0,0,.5,.5)], '▏': [(0,0,.125,1)]}
for y, row in enumerate(rows):
    for x in range(W):
        ch, fg, bg, bold = row[x] if x < len(row) else (' ', None, None, False)
        fg = fg or DEF_FG; bgc = bg or DEF_BG
        X, Y = x*CW, y*CH
        out.append(f'<div style="position:absolute;left:{X}px;top:{Y}px;width:{CW}px;height:{CH}px;background:{bgc}"></div>')
        if ch in BLK:
            for (a, b_, w, h) in BLK[ch]:
                out.append(f'<div style="position:absolute;left:{X+a*CW}px;top:{Y+b_*CH}px;width:{w*CW}px;height:{h*CH}px;background:{fg}"></div>')
        elif ch == '':
            out.append(f'<div style="position:absolute;left:{X}px;top:{Y}px;width:{CW}px;height:{CH}px;background:{fg};border-radius:{CW}px 0 0 {CW}px"></div>')
        elif ch == '':
            out.append(f'<div style="position:absolute;left:{X}px;top:{Y}px;width:{CW}px;height:{CH}px;background:{fg};border-radius:0 {CW}px {CW}px 0"></div>')
        elif ch in '─│╭╮╰╯':
            mx, my = X + CW//2, Y + CH//2
            if ch == '─': out.append(f'<div style="position:absolute;left:{X}px;top:{my}px;width:{CW}px;height:1px;background:{fg}"></div>')
            elif ch == '│': out.append(f'<div style="position:absolute;left:{mx}px;top:{Y}px;width:1px;height:{CH}px;background:{fg}"></div>')
            else:
                r = CW//2
                L = mx if ch in '╭╰' else X
                T = my if ch in '╭╮' else Y
                w = (X+CW-mx) if ch in '╭╰' else (mx-X+1)
                h = (Y+CH-my) if ch in '╭╮' else (my-Y+1)
                bs = {'╭': f'border-top:1px solid {fg};border-left:1px solid {fg};border-top-left-radius:{r}px',
                      '╮': f'border-top:1px solid {fg};border-right:1px solid {fg};border-top-right-radius:{r}px',
                      '╰': f'border-bottom:1px solid {fg};border-left:1px solid {fg};border-bottom-left-radius:{r}px',
                      '╯': f'border-bottom:1px solid {fg};border-right:1px solid {fg};border-bottom-right-radius:{r}px'}[ch]
                out.append(f'<div style="position:absolute;left:{L}px;top:{T}px;width:{w}px;height:{h}px;box-sizing:border-box;{bs}"></div>')
        elif ch.strip():
            pua = 0xE000 <= ord(ch) <= 0xF8FF
            wide = pua and x+1 < len(row) and row[x+1][0] == ' '
            span_w = CW*2 if wide else CW
            fs = FS*1.15 if wide else FS
            fw = 'bold' if bold else 'normal'
            out.append(f'<div style="position:absolute;left:{X}px;top:{Y}px;width:{span_w}px;height:{CH}px;line-height:{CH}px;text-align:center;color:{fg};font-weight:{fw};font-size:{fs}px;font-family:JBM,NFS;z-index:2">{html.escape(ch)}</div>')
print(f'''<html><head><style>
@font-face{{font-family:JBM;src:url(file://{D}/jbm/fonts/ttf/JetBrainsMono-Regular.ttf)}}
@font-face{{font-family:JBM;font-weight:bold;src:url(file://{D}/jbm/fonts/ttf/JetBrainsMono-Bold.ttf)}}
@font-face{{font-family:NFS;src:url(file://{D}/nfsym/SymbolsNerdFont-Regular.ttf)}}
</style></head><body style="margin:0;background:{DEF_BG}"><div style="position:relative;width:{W*CW}px;height:{len(rows)*CH}px">{"".join(out)}</div></body></html>''')
