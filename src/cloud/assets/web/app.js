// tasq web: your calendar, read-only. The server works everything out
// (occurrences, repeats, lanes, free gaps, colours, notes as HTML); this
// only paints it. Phone first: a week strip, Agenda / Day / Month tabs,
// swipes, and a sheet with the details. On a computer: the week, a side
// panel, and vim-style keys (every one of them also a button).
(() => {
  'use strict';

  // ── words ──────────────────────────────────────────────
  const LANG = (navigator.language || 'en').toLowerCase().startsWith('es') ? 'es' : 'en';
  const WORDS = {
    en: {
      today: 'Today', agenda: 'Agenda', day: 'Day', week: 'Week', month: 'Month',
      allDay: 'All day', free: 'Free time', noFree: 'No gaps between 08 and 22',
      nothing: 'Nothing planned', nothingDay: 'Nothing on this day', late: 'Late',
      deadline: 'Deadline', repeat: 'Repeats', upcoming: 'Upcoming repeat', due: 'Due',
      priority: 'Priority', tags: 'Tags', done: 'Done', doneOn: 'Done on', subtasks: 'Subtasks',
      notes: 'Notes', task: 'Task', event: 'Event', join: 'Join video call', openGoogle: 'Open in Google Calendar',
      where: 'Where', people: 'People', files: 'Files', about: 'Details', tentative: 'You haven\'t said yes yet',
      maybe: 'You said maybe', readOnly: 'Read-only here · change it in tasq',
      readOnlyExt: 'From another calendar · read-only',
      settings: 'Settings', theme: 'Theme', myThemes: 'Your themes',
      myThemesSub: 'Themes of your own (the files in tasq\'s themes folder) show up here once a computer with them syncs.', fullDay: 'Show all 24 hours',
      fullDaySub: 'Otherwise 07–22, widened when something falls outside',
      titles: 'Titles in the month', titlesSub: 'Instead of dots, on wide screens',
      install: 'Add to your home screen', installIos: 'In Safari: Share → Add to Home Screen.',
      installBtn: 'Install', signOut: 'Sign out of this browser',
      signOutSub: 'You can also disconnect it in tasq → Settings → Sync.',
      keys: 'Keyboard shortcuts', keysSub: 'Everything also has a button: these are only shortcuts.',
      kPrevNext: 'Previous / next', kToday: 'Today', kViews: 'Day · week · month · agenda',
      kMove: 'Next / previous item', kOpen: 'Open the details', kClose: 'Close', kRefresh: 'Refresh',
      kHelp: 'This list', kSettings: 'Settings', kHours: 'All 24 hours',
      offline: 'No connection: showing what was last loaded', updated: 'Up to date',
      showAll: 'Show all 24 hours', showLess: 'Show 07–22', more: 'more',
      unavailable: 'Couldn\'t read', dotsMode: '• Dots', titlesMode: 'Aa Titles', items: n => n === 1 ? '1 item' : `${n} items`,
      now: 'now', min: 'min', on: 'on', every: 'every', strict: 'from the last date',
      units: { d: ['day', 'days'], b: ['weekday', 'weekdays'], w: ['week', 'weeks'], m: ['month', 'months'], y: ['year', 'years'] },
      wd: ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'], dayOf: (a, b) => `day ${a} of ${b}`,
      wasOn: d => `was on ${d}`, prio: { A: 'high', B: 'medium', C: 'low', D: 'lowest' },
      accepted: 'yes', declined: 'no', needsAction: 'no answer', organizer: 'organizer',
    },
    es: {
      today: 'Hoy', agenda: 'Agenda', day: 'Día', week: 'Semana', month: 'Mes',
      allDay: 'Todo el día', free: 'Huecos libres', noFree: 'Sin huecos entre las 08 y las 22',
      nothing: 'Nada previsto', nothingDay: 'Nada este día', late: 'Atrasada',
      deadline: 'Fecha límite', repeat: 'Se repite', upcoming: 'Repetición futura', due: 'Fecha límite',
      priority: 'Prioridad', tags: 'Etiquetas', done: 'Hecha', doneOn: 'Hecha el', subtasks: 'Subtareas',
      notes: 'Notas', task: 'Tarea', event: 'Evento', join: 'Unirse a la videollamada', openGoogle: 'Abrir en Google Calendar',
      where: 'Dónde', people: 'Invitados', files: 'Archivos', about: 'Detalles', tentative: 'Aún no has respondido',
      maybe: 'Respondiste quizá', readOnly: 'Solo lectura · cámbialo en tasq',
      readOnlyExt: 'De otro calendario · solo lectura',
      settings: 'Ajustes', theme: 'Tema', myThemes: 'Tus temas',
      myThemesSub: 'Tus temas propios (los archivos de la carpeta de temas de tasq) aparecen aquí cuando sincroniza un ordenador que los tenga.', fullDay: 'Ver las 24 horas',
      fullDaySub: 'Si no, de 07 a 22, ampliado si algo cae fuera',
      titles: 'Títulos en el mes', titlesSub: 'En lugar de puntos, en pantallas anchas',
      install: 'Añadir a la pantalla de inicio', installIos: 'En Safari: Compartir → Añadir a pantalla de inicio.',
      installBtn: 'Instalar', signOut: 'Cerrar sesión en este navegador',
      signOutSub: 'También puedes desconectarlo en tasq → Ajustes → Sync.',
      keys: 'Atajos de teclado', keysSub: 'Todo tiene también su botón: esto son solo atajos.',
      kPrevNext: 'Anterior / siguiente', kToday: 'Hoy', kViews: 'Día · semana · mes · agenda',
      kMove: 'Siguiente / anterior', kOpen: 'Abrir los detalles', kClose: 'Cerrar', kRefresh: 'Actualizar',
      kHelp: 'Esta lista', kSettings: 'Ajustes', kHours: 'Las 24 horas',
      offline: 'Sin conexión: se muestra lo último que cargó', updated: 'Actualizado',
      showAll: 'Ver las 24 horas', showLess: 'Ver de 07 a 22', more: 'más',
      unavailable: 'No se pudo leer', dotsMode: '• Puntos', titlesMode: 'Aa Títulos', items: n => n === 1 ? '1 cosa' : `${n} cosas`,
      now: 'ahora', min: 'min', on: 'los', every: 'cada', strict: 'desde la última fecha',
      units: { d: ['día', 'días'], b: ['día laborable', 'días laborables'], w: ['semana', 'semanas'], m: ['mes', 'meses'], y: ['año', 'años'] },
      wd: ['lun', 'mar', 'mié', 'jue', 'vie', 'sáb', 'dom'], dayOf: (a, b) => `día ${a} de ${b}`,
      wasOn: d => `era el ${d}`, prio: { A: 'alta', B: 'media', C: 'baja', D: 'mínima' },
      accepted: 'sí', declined: 'no', needsAction: 'sin respuesta', organizer: 'organiza',
    },
  };
  const W = WORDS[LANG];
  document.documentElement.lang = LANG;

  // ── small helpers ──────────────────────────────────────
  const $ = (s) => document.querySelector(s);
  const TZ = (() => { try { return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'; } catch (e) { return 'UTC'; } })();
  const keep = {
    get(k, d) { try { const v = localStorage.getItem('tasq.' + k); return v === null ? d : JSON.parse(v); } catch (e) { return d; } },
    set(k, v) { try { localStorage.setItem('tasq.' + k, JSON.stringify(v)); } catch (e) { /* private mode */ } },
  };
  const pad = (n) => String(n).padStart(2, '0');
  const parse = (s) => { const [y, m, d] = s.split('-').map(Number); return new Date(Date.UTC(y, m - 1, d)); };
  const iso = (d) => d.toISOString().slice(0, 10);
  const add = (s, n) => { const d = parse(s); d.setUTCDate(d.getUTCDate() + n); return iso(d); };
  const addMonths = (s, n) => { const d = parse(s); d.setUTCDate(1); d.setUTCMonth(d.getUTCMonth() + n); return iso(d); };
  const weekday = (s) => (parse(s).getUTCDay() + 6) % 7;
  const monday = (s) => add(s, -weekday(s));
  const localToday = () => { const n = new Date(); return `${n.getFullYear()}-${pad(n.getMonth() + 1)}-${pad(n.getDate())}`; };
  const nowMinute = () => { const n = new Date(); return n.getHours() * 60 + n.getMinutes(); };
  const hhmm = (m) => `${pad(Math.floor(m / 60) % 24)}:${pad(m % 60)}`;
  const dur = (m) => { const h = Math.floor(m / 60), r = m % 60; return h ? (r ? `${h}h ${r}m` : `${h}h`) : `${r}m`; };
  const fmtDate = (s, o) => new Intl.DateTimeFormat(LANG, { timeZone: 'UTC', ...o }).format(parse(s));
  const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1);
  // "Noviembre 2026", without the "de".
  const monthYear = (s) => {
    const parts = new Intl.DateTimeFormat(LANG, { timeZone: 'UTC', month: 'long', year: 'numeric' }).formatToParts(parse(s));
    const get = (t) => (parts.find((p) => p.type === t) || {}).value || '';
    return cap(`${get('month')} ${get('year')}`);
  };
  // Where `el` sits inside the scrolling `main`.
  const offsetIn = (el, main) => el.getBoundingClientRect().top - main.getBoundingClientRect().top + main.scrollTop;
  const isWide = () => matchMedia('(min-width: 760px)').matches;
  const hasSide = () => matchMedia('(min-width: 1100px)').matches;

  // Build elements: h('div.cls', {attrs}, ...children).
  function h(tag, attrs, ...kids) {
    const [name, ...cls] = tag.split('.');
    const el = document.createElement(name || 'div');
    if (cls.length) el.className = cls.join(' ');
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v == null || v === false) continue;
      if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
      else if (k.startsWith('--')) el.style.setProperty(k, v);
      else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
      else if (k === 'html') el.innerHTML = v;
      else el.setAttribute(k, v === true ? '' : v);
    }
    for (const k of kids.flat()) if (k != null && k !== false) el.append(k.nodeType ? k : String(k));
    return el;
  }

  // Text with its web addresses as links.
  function linkify(text) {
    const out = document.createDocumentFragment();
    const re = /https?:\/\/[^\s<>"')]+/g;
    let at = 0, m;
    while ((m = re.exec(text))) {
      out.append(text.slice(at, m.index));
      out.append(h('a', { href: m[0], target: '_blank', rel: 'noopener noreferrer' }, m[0]));
      at = m.index + m[0].length;
    }
    out.append(text.slice(at));
    return out;
  }

  let toastTimer;
  function toast(msg) {
    const el = $('#toast');
    el.textContent = msg; el.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { el.hidden = true; }, 2600);
  }

  // ── state ──────────────────────────────────────────────
  const S = {
    view: keep.get('view', null),
    cur: localToday(),
    sel: null,
    theme: null,
    themes: [],
    full: keep.get('full', false),
    titles: keep.get('titles', true),
    days: new Map(),
    details: {},
    fetched: new Map(),
    unavailable: {},
    offline: false,
    scrolled: null,
    sheet: null,
  };
  if (!S.view) S.view = isWide() ? 'week' : 'agenda';

  // What actually shows: no week grid on a phone.
  const view = () => (!isWide() && S.view === 'week' ? 'agenda' : S.view);

  // ── theme ──────────────────────────────────────────────
  const DEFAULT_THEME = 'Catppuccin Macchiato';
  function applyTheme() {
    const th = S.themes.find((x) => x.name === keep.get('theme', DEFAULT_THEME)) || S.themes[0];
    if (!th) return;
    S.theme = th;
    const r = document.documentElement.style;
    const set = { bg: th.bg, panel: th.panel, border: th.border, fg: th.fg, dim: th.dim, accent: th.accent,
      sel: th.selected, today: th.today, overdue: th.overdue, due: th.due, ok: th.ok, done: th.done, 'on-accent': th.mode_fg };
    for (const [k, v] of Object.entries(set)) r.setProperty('--' + k, v);
    const light = lum(th.bg) > 0.5;
    document.documentElement.style.colorScheme = light ? 'light' : 'dark';
    document.querySelector('meta[name=theme-color]').setAttribute('content', th.panel);
  }
  function lum(hex) {
    const n = parseInt(hex.slice(1), 16);
    return (0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255)) / 255;
  }
  // An item's colour: its space's or calendar's, red when late or due.
  function colorOf(it) {
    const th = S.theme;
    if (!th) return '#888';
    if (it.kind !== 'external' && (it.late || it.deadline)) return th.overdue;
    const c = it.color;
    if (c == null) return th.accent;
    if (c[0] === '#') return c;
    return th.palette[Number(c) % th.palette.length] || th.accent;
  }

  // ── data ───────────────────────────────────────────────
  function range() {
    const v = view();
    if (v === 'month') {
      const first = S.cur.slice(0, 8) + '01';
      const from = monday(first);
      return [from, add(from, 41)];
    }
    const from = monday(S.cur);
    return [from, add(from, 6)];
  }

  let inflight = 0;
  async function load(force) {
    const [from, to] = range();
    const key = from + '|' + to;
    const at = S.fetched.get(key);
    if (!force && at && Date.now() - at < 60_000) return;
    inflight++;
    $('#app').classList.add('busy');
    showLoading(true);
    try {
      const res = await fetch(`/v1/calendar?from=${from}&to=${to}&tz=${encodeURIComponent(TZ)}`, {
        credentials: 'same-origin', cache: 'no-store',
      });
      if (res.status === 401) { location.href = '/login'; return; }
      if (!res.ok) throw new Error((await res.json().catch(() => ({}))).error || res.status);
      const data = await res.json();
      for (const d of data.days) S.days.set(d.date, d);
      Object.assign(S.details, data.details);
      S.unavailable = data.calendars_unavailable || {};
      if (Array.isArray(data.themes) && data.themes.length) mergeThemes(data.themes);
      S.fetched.set(key, Date.now());
      S.offline = false;
      render();
    } catch (e) {
      if (!S.offline) toast(W.offline);
      S.offline = true;
      render();
    } finally {
      inflight--;
      if (!inflight) showLoading(false);
    }
  }
  function showLoading(on) {
    let el = document.querySelector('.loading');
    if (on && !el) document.body.append(h('div.loading'));
    if (!on && el) el.remove();
  }
  // Themes of your own, from your account: added to the list.
  function mergeThemes(list) {
    for (const t of list) {
      t.mine = true;
      const i = S.themes.findIndex((x) => x.name === t.name);
      if (i >= 0) S.themes[i] = t; else S.themes.push(t);
    }
    applyTheme();
  }

  const dayData = (d) => S.days.get(d) || { date: d, all_day: [], timed: [], free: [] };
  const itemsOf = (d) => { const x = dayData(d); return [...x.all_day, ...x.timed]; };

  // ── navigation ─────────────────────────────────────────
  function go(date, opts = {}) {
    S.cur = date;
    if (!opts.keepSel) S.sel = null;
    S.scrolled = null;
    render();
    load();
  }
  function step(n) {
    const v = view();
    if (v === 'month') go(addMonths(S.cur, n));
    else if (v === 'day') go(add(S.cur, n));
    else go(add(S.cur, 7 * n));
  }
  function setView(v) {
    S.view = v;
    keep.set('view', v);
    S.sel = null;
    S.scrolled = null;
    render();
    load();
  }
  function select(it, date, open) {
    S.sel = it ? { key: it.key, date, start: it.start, origin: it.origin } : null;
    if (date && date !== S.cur) S.cur = date;
    render();
    if (it && (open || !hasSide())) openDetail();
  }

  // ── rendering ──────────────────────────────────────────
  function render() {
    renderTop();
    renderStrip();
    $('#strip').hidden = view() === 'month';
    renderTabs();
    const main = $('#main');
    const prevScroll = main.scrollTop;
    main.replaceChildren();
    const v = view();
    if (Object.keys(S.unavailable).length) {
      main.append(h('div.banner', {}, `${W.unavailable}: ${Object.keys(S.unavailable).join(', ')}`));
    }
    if (v === 'agenda') renderAgenda(main);
    else if (v === 'day') renderDay(main);
    else if (v === 'week') renderWeek(main);
    else renderMonth(main);
    renderSide();
    if (S.scrolled === v + S.cur) main.scrollTop = prevScroll;
    else { S.scrolled = v + S.cur; scrollToStart(main, v); }
    if (S.sheet === 'detail') fillDetail($('#sheet-body'));
  }

  function scrollToStart(main, v) {
    if (v === 'agenda') {
      const el = main.querySelector(`[data-day="${S.cur}"]`);
      if (el) main.scrollTop = offsetIn(el, main);
    } else if (v === 'day' || v === 'week') {
      const tl = main.querySelector('.timeline');
      if (!tl) return;
      const h0 = Number(tl.dataset.h0);
      const today = localToday();
      const showsToday = v === 'day' ? S.cur === today : range()[0] <= today && today <= range()[1];
      let m = showsToday ? nowMinute() - 60 : 8 * 60;
      if (!showsToday) {
        const firsts = (v === 'day' ? [S.cur] : weekDays()).flatMap((d) => dayData(d).timed.map((x) => x.start));
        if (firsts.length) m = Math.min(...firsts) - 30;
      }
      const head = main.querySelector('.week-sticky');
      const top = offsetIn(tl, main) + Math.max(0, (m / 60 - h0)) * hourPx() - (head ? head.offsetHeight : 0);
      main.scrollTop = Math.max(0, top - 8);
    }
  }

  function renderTop() {
    const v = view();
    let title;
    if (v === 'month' || !isWide()) title = monthYear(S.cur);
    else if (v === 'day') title = cap(fmtDate(S.cur, { weekday: 'long', day: 'numeric', month: 'short', year: 'numeric' }));
    else {
      const [a, b] = range();
      title = a.slice(0, 7) === b.slice(0, 7)
        ? `${Number(a.slice(8))} – ${fmtDate(b, { day: 'numeric', month: 'short', year: 'numeric' })}`
        : `${fmtDate(a, { day: 'numeric', month: 'short' })} – ${fmtDate(b, { day: 'numeric', month: 'short', year: 'numeric' })}`;
    }
    $('#title-text').textContent = title;
    $('#today').textContent = W.today;
    $('#prev').title = `${W.kPrevNext} (h)`;
    $('#next').title = `${W.kPrevNext} (l)`;
    $('#today').title = `${W.kToday} (t)`;
    $('#refresh').title = `${W.kRefresh} (r)`;
    $('#help').title = `${W.keys} (?)`;
    $('#menu').title = `${W.settings} (,)`;
    const views = $('#views');
    views.replaceChildren(...[['day', 'd'], ['week', 'w'], ['month', 'm'], ['agenda', 'a']].map(([k, key]) =>
      h('button', { type: 'button', role: 'tab', 'aria-selected': String(v === k), title: `${W[k]} (${key})`, onclick: () => setView(k) }, W[k])));
  }

  function renderTabs() {
    const v = view();
    const tabs = [['agenda', '☰'], ['day', '▤'], ['month', '▦']];
    $('#tabs').replaceChildren(...tabs.map(([k, ic]) =>
      h('button', { type: 'button', role: 'tab', 'aria-selected': String(v === k), onclick: () => setView(k) },
        h('span.ic', {}, ic), W[k])));
  }

  function dotsFor(d, max) {
    const items = itemsOf(d);
    return h('span.dots', {}, items.slice(0, max).map((it) => h('i.dot', { '--c': colorOf(it) })));
  }

  function renderStrip() {
    const start = monday(S.cur);
    const today = localToday();
    const strip = $('#strip');
    strip.replaceChildren(...Array.from({ length: 7 }, (_, i) => {
      const d = add(start, i);
      const cls = [d === today && 'is-today', d === S.cur && 'is-cur'].filter(Boolean).join(' ');
      return h('button', { type: 'button', class: cls, 'aria-label': fmtDate(d, { weekday: 'long', day: 'numeric', month: 'long' }),
        'aria-current': d === S.cur ? 'date' : null, onclick: () => go(d) },
        h('span.wd', {}, fmtDate(d, { weekday: 'narrow' })),
        h('span.n', {}, Number(d.slice(8))),
        dotsFor(d, 4));
    }));
  }

  // One item as a row (agenda, lists).
  function itemRow(it, date) {
    const c = colorOf(it);
    const sel = S.sel && S.sel.key === it.key && S.sel.date === date;
    const when = it.all_day
      ? h('span.when.ad', {}, it.late ? W.late : it.deadline ? W.due : (it.span > 1 ? `${it.day + 1}/${it.span}` : W.allDay))
      : h('span.when', {}, hhmm(it.start), h('small', {}, dur(it.minutes)));
    const barCls = it.kind === 'external' ? (it.tentative ? 'bar hollow' : 'bar') : it.projected ? 'bar dashed' : 'bar';
    const sub = [];
    if (it.label) sub.push(h('span.lab', {}, it.label));
    if (!it.all_day) sub.push(` · ${hhmm(it.start)}–${hhmm(it.end)}`);
    if (it.span > 1) sub.push(` · ${W.dayOf(it.day + 1, it.span)}`);
    if (it.projected) sub.push(` · ↻ ${W.upcoming.toLowerCase()}`);
    if (it.tentative) sub.push(` · ${W.tentative.toLowerCase()}`);
    return h('button', { type: 'button', class: `row${it.done ? ' done' : ''}${sel ? ' is-sel' : ''}`, '--c': c,
      onclick: () => select(it, date, true) },
      when, h('span', { class: barCls }),
      h('span.what', {}, h('div.t', {}, titleNodes(it)), sub.length ? h('div.sub', {}, sub) : null));
  }

  function titleNodes(it) {
    const out = [];
    if (it.kind === 'task') out.push(h('span.box', { 'aria-hidden': 'true' }, it.done ? '✓' : ''));
    if (it.kind === 'event') out.push('◆ ');
    if (it.kind === 'external' && it.tentative) out.push('◌ ');
    if (it.projected) out.push('↻ ');
    out.push(it.title);
    if (it.priority) out.push(h('span.tag', { '--c': S.theme && S.theme.priority[it.priority] }, it.priority));
    return out;
  }

  function renderAgenda(main) {
    const start = monday(S.cur);
    const today = localToday();
    for (let i = 0; i < 7; i++) {
      const d = add(start, i);
      const items = itemsOf(d);
      const head = h('div', { class: `day-h${d === today ? ' is-today' : ''}` },
        h('span.d', {}, fmtDate(d, { weekday: 'long', day: 'numeric', month: 'short' })),
        d === today ? h('span.tag', { '--c': 'var(--today)' }, W.today) : null,
        h('span.r', {}, items.length ? W.items(items.length) : ''));
      const box = h('section.agenda-day', { 'data-day': d }, head);
      if (!items.length) box.append(h('div.none', {}, W.nothing));
      const dd = dayData(d);
      for (const it of dd.all_day) box.append(itemRow(it, d));
      let nowShown = d !== today;
      for (const it of dd.timed) {
        if (!nowShown && it.start > nowMinute()) { box.append(h('div.now-row', {}, hhmm(nowMinute()))); nowShown = true; }
        box.append(itemRow(it, d));
      }
      if (!nowShown && dd.timed.length) box.append(h('div.now-row', {}, hhmm(nowMinute())));
      main.append(box);
    }
    swipe(main, (dir) => step(dir));
  }

  // ── timeline (day, week) ───────────────────────────────
  const hourPx = () => parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--hour')) || 56;
  function hourSpan(days) {
    if (S.full) return [0, 24];
    let h0 = 7, h1 = 22;
    for (const d of days) for (const it of dayData(d).timed) {
      h0 = Math.min(h0, Math.floor(it.start / 60));
      h1 = Math.max(h1, Math.ceil(Math.min(it.end, 1440) / 60));
    }
    return [h0, h1];
  }

  function timeline(days, cols) {
    const [h0, h1] = hourSpan(days);
    const px = hourPx();
    const height = (h1 - h0) * px;
    const hours = h('div.hours', { style: { height: height + 'px' } });
    for (let hr = h0 + 1; hr < h1; hr++) hours.append(h('span', { style: { top: (hr - h0) * px + 'px' } }, pad(hr)));
    const grid = h('div.cols', { style: { gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, height: height + 'px' } });
    const today = localToday();
    for (const d of days) {
      const col = h('div', { class: `col${d === today && cols > 1 ? ' is-today' : ''}` });
      for (let hr = h0; hr < h1; hr++) {
        col.append(h('div.hline', { style: { top: (hr - h0) * px + 'px' } }));
        col.append(h('div', { class: 'hline half', style: { top: (hr - h0 + 0.5) * px + 'px' } }));
      }
      for (const it of dayData(d).timed) col.append(block(it, d, h0, px, cols));
      if (d === today) {
        const m = nowMinute();
        if (m >= h0 * 60 && m <= h1 * 60) col.append(h('div.nowline', { style: { top: ((m / 60) - h0) * px + 'px' } }));
      }
      grid.append(col);
    }
    return h('div.timeline', { 'data-h0': h0 }, hours, grid);
  }

  function block(it, d, h0, px, cols) {
    const c = colorOf(it);
    const top = ((it.start / 60) - h0) * px;
    const hgt = Math.max((Math.min(it.end, 1440) - it.start) / 60 * px, 18) - 2;
    const w = 100 / it.lanes;
    const sel = S.sel && S.sel.key === it.key && S.sel.date === d && S.sel.start === it.start;
    const cls = ['block', it.kind === 'external' && 'ext', it.tentative && 'tentative', it.projected && 'projected',
      it.done && 'done', sel && 'is-sel', hgt < 34 && 'tiny'].filter(Boolean).join(' ');
    const detail = [`${hhmm(it.start)}–${hhmm(it.end)}`, it.label].filter(Boolean).join(' · ');
    return h('button', { type: 'button', class: cls, '--c': c, title: `${it.title}\n${detail}`,
      style: { top: top + 1 + 'px', height: hgt + 'px', left: `calc(${it.lane * w}% + 2px)`, width: `calc(${w}% - 4px)` },
      onclick: () => select(it, d) },
      h('span.bt', {}, prefix(it), it.title, hgt < 34 && cols === 1 ? h('span.muted', {}, ` · ${hhmm(it.start)}`) : null),
      h('span.bd', {}, detail));
  }
  function prefix(it) {
    let p = '';
    if (it.projected) p += '↻ ';
    if (it.kind === 'event') p += '◆ ';
    if (it.kind === 'external' && it.tentative) p += '◌ ';
    if (it.priority) p += `(${it.priority}) `;
    return p;
  }

  function chip(it, d, extra = {}) {
    const sel = S.sel && S.sel.key === it.key && S.sel.date === d;
    const cls = ['chip', it.kind === 'external' && 'ext', it.tentative && 'tentative', it.projected && 'dashed',
      it.done && 'done', sel && 'is-sel', extra.cls].filter(Boolean).join(' ');
    let text = prefix(it) + it.title;
    if (it.late) text = `${W.late} · ${text}`;
    else if (it.deadline) text = `${W.due} · ${text}`;
    if (it.span > 1 && !extra.band) text += ` · ${it.day + 1}/${it.span}`;
    return h('button', { type: 'button', class: cls, '--c': colorOf(it), style: extra.style, title: it.title,
      onclick: () => select(it, d) }, h('span', {}, text));
  }

  function renderDay(main) {
    const d = S.cur;
    const dd = dayData(d);
    if (!isWide()) {
      main.append(h('div', { class: `day-h${d === localToday() ? ' is-today' : ''}` },
        h('span.d', {}, fmtDate(d, { weekday: 'long', day: 'numeric', month: 'long' })),
        h('span.r', {}, W.items(itemsOf(d).length))));
    }
    if (dd.all_day.length) {
      main.append(h('div.allday', {}, dd.all_day.map((it) => chip(it, d))));
    }
    main.append(timeline([d], 1));
    main.append(h('button.expand', { type: 'button', onclick: toggleFull }, S.full ? W.showLess : W.showAll));
    if (!hasSide()) {
      main.append(h('div.section-h', {}, W.free));
      main.append(freeList(dd));
    }
    swipe(main, (dir) => step(dir));
  }

  function freeList(dd) {
    if (!dd.free.length) return h('div.free-list', {}, h('div.muted', {}, W.noFree));
    return h('div.free-list', {}, dd.free.map(([s, e]) => h('div.f', {}, h('span', {}, `${hhmm(s)} – ${hhmm(e)}`), h('span', {}, dur(e - s)))));
  }

  const weekDays = () => Array.from({ length: 7 }, (_, i) => add(monday(S.cur), i));

  function renderWeek(main) {
    const days = weekDays();
    const today = localToday();
    const cols = `52px repeat(7, minmax(0, 1fr))`;
    const sticky = h('div.week-sticky');
    main.append(sticky);
    sticky.append(h('div.week-head', { style: { gridTemplateColumns: cols } }, h('div'),
      days.map((d) => h('button', { type: 'button', class: [d === today && 'is-today', d === S.cur && 'is-cur'].filter(Boolean).join(' '),
        onclick: () => { S.cur = d; setView('day'); } },
        h('div.wd', {}, fmtDate(d, { weekday: 'short' })), h('div.n', {}, Number(d.slice(8)))))));
    // All-day: bands across the days they cover, packed by the grid.
    const bands = [];
    const seen = new Set();
    days.forEach((d, i) => {
      for (const it of dayData(d).all_day) {
        if (it.span > 1) {
          const k = it.key + '|' + it.origin + '|' + it.projected;
          if (seen.has(k)) continue;
          seen.add(k);
          let last = i;
          for (let j = i + 1; j < 7; j++) if (dayData(days[j]).all_day.some((x) => x.key === it.key && x.origin === it.origin)) last = j;
          bands.push({ it, d, c0: i, c1: last, cont: it.day > 0 });
        } else bands.push({ it, d, c0: i, c1: i });
      }
    });
    if (bands.length) {
      bands.sort((a, b) => (b.c1 - b.c0) - (a.c1 - a.c0) || a.c0 - b.c0);
      sticky.append(h('div.week-allday', { style: { gridTemplateColumns: cols, gridAutoFlow: 'row dense' } },
        bands.map((b) => chip(b.it, b.d, { band: true, cls: b.cont ? 'cont-l' : '', style: { gridColumn: `${b.c0 + 2} / ${b.c1 + 3}` } }))));
    }
    main.append(timeline(days, 7));
    main.append(h('button.expand', { type: 'button', onclick: toggleFull }, S.full ? W.showLess : W.showAll));
  }

  function renderMonth(main) {
    const [from] = range();
    const month = S.cur.slice(0, 7);
    const today = localToday();
    const titles = isWide() && S.titles;
    main.append(h('div.month-head', {}, Array.from({ length: 7 }, (_, i) => h('div', {}, fmtDate(add(from, i), { weekday: isWide() ? 'short' : 'narrow' })))));
    const grid = h('div.month');
    for (let i = 0; i < 42; i++) {
      const d = add(from, i);
      const items = itemsOf(d);
      const cls = ['cell', d.slice(0, 7) !== month && 'other', d === today && 'is-today', d === S.cur && 'is-cur'].filter(Boolean).join(' ');
      const kids = [h('span.n', {}, Number(d.slice(8)))];
      if (titles) {
        for (const it of items.slice(0, 4)) {
          kids.push(h('span', { class: `mt${it.deadline || it.late ? ' deadline' : ''}`, '--c': colorOf(it) },
            (it.all_day ? '' : hhmm(it.start) + ' ') + it.title));
        }
        if (items.length > 4) kids.push(h('span.more', {}, `+${items.length - 4} ${W.more}`));
      } else if (items.length) {
        kids.push(dotsFor(d, 8));
        kids.push(h('span.cnt', {}, items.length));
      }
      grid.append(h('button', { type: 'button', class: cls, 'aria-label': `${fmtDate(d, { weekday: 'long', day: 'numeric', month: 'long' })}: ${W.items(items.length)}`,
        onclick: () => { if (S.cur === d && isWide()) { setView('day'); } else go(d); } }, kids));
    }
    main.append(grid);
    // The chosen day's list underneath.
    const box = h('section.month-day', {}, h('div.day-h', {}, h('span.d', {}, fmtDate(S.cur, { weekday: 'long', day: 'numeric', month: 'long' }))));
    const items = itemsOf(S.cur);
    if (!items.length) box.append(h('div.empty', {}, W.nothingDay));
    for (const it of items) box.append(itemRow(it, S.cur));
    main.append(box);
    if (isWide()) {
      main.prepend(h('div', { style: { display: 'flex', justifyContent: 'flex-end', padding: '8px 16px 0' } },
        h('button', { type: 'button', class: 'pill-btn month-toggle', onclick: () => { S.titles = !S.titles; keep.set('titles', S.titles); render(); } },
          S.titles ? W.dotsMode : W.titlesMode)));
    }
    swipe(main, (dir) => step(dir));
  }

  function toggleFull() {
    S.full = !S.full;
    keep.set('full', S.full);
    S.scrolled = null;
    render();
  }

  // ── side panel & details ───────────────────────────────
  function selectedItem() {
    if (!S.sel) return null;
    return itemsOf(S.sel.date).find((x) => x.key === S.sel.key && x.start === S.sel.start)
      || itemsOf(S.sel.date).find((x) => x.key === S.sel.key) || null;
  }

  function renderSide() {
    const side = $('#side');
    if (!hasSide()) { side.replaceChildren(); return; }
    side.replaceChildren();
    if (S.sel && selectedItem()) {
      fillDetail(side);
      return;
    }
    const dd = dayData(S.cur);
    side.append(h('div.det-kicker', {}, fmtDate(S.cur, { weekday: 'long' })));
    side.append(h('h2.det-title', {}, fmtDate(S.cur, { day: 'numeric', month: 'long', year: 'numeric' })));
    side.append(h('div.det-when', {}, W.items(itemsOf(S.cur).length)));
    side.append(h('div.set-h', {}, W.free));
    side.append(freeList(dd));
  }

  function openDetail() {
    if (hasSide() && view() !== 'month') return;
    S.sheet = 'detail';
    fillDetail($('#sheet-body'));
    showSheet();
  }

  function repeatLabel(rec) {
    if (!rec) return null;
    const m = /^(\+?)(\d+)([dbwmy])(?::(.+))?$/.exec(rec);
    if (!m) return rec;
    const n = Number(m[2]);
    const [one, many] = W.units[m[3]];
    let s = n === 1 ? `${W.every} ${one}` : `${W.every} ${n} ${many}`;
    if (m[4]) {
      const names = { mon: 0, tue: 1, wed: 2, thu: 3, fri: 4, sat: 5, sun: 6 };
      s += ` · ${W.on} ` + m[4].split(',').map((x) => W.wd[names[x]] ?? x).join(', ');
    }
    if (m[1]) s += ` (${W.strict})`;
    return s;
  }

  function fillDetail(root) {
    root.replaceChildren();
    const it = selectedItem();
    if (!it) return;
    const det = S.details[it.key] || {};
    const c = colorOf(it);
    const d = S.sel.date;
    const box = h('div', { '--c': c });
    if (it.kind === 'external') {
      box.append(h('div.det-kicker', {}, det.calendar || W.event, det.account ? h('span.muted', {}, ` · ${det.account}`) : null));
    } else {
      box.append(h('div.det-kicker', {}, det.space || (it.kind === 'event' ? W.event : W.task)));
    }
    box.append(h('h2.det-title', { style: det.done ? { textDecoration: 'line-through', color: 'var(--done)' } : null }, prefix(it), it.title));
    const dayText = fmtDate(d, { weekday: 'long', day: 'numeric', month: 'long' });
    let when = it.all_day ? `${dayText} · ${W.allDay}` : `${dayText} · ${hhmm(it.start)}–${hhmm(it.end)} (${dur(it.minutes)})`;
    if (it.kind === 'external' && det.starts && !det.all_day) {
      const f = new Intl.DateTimeFormat(LANG, { weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit', timeZone: TZ });
      const tf = new Intl.DateTimeFormat(LANG, { hour: '2-digit', minute: '2-digit', timeZone: TZ });
      const a = new Date(det.starts), b = new Date(det.ends);
      when = a.toDateString() === b.toDateString() ? `${f.format(a)}–${tf.format(b)}` : `${f.format(a)} → ${f.format(b)}`;
    }
    if (it.span > 1) when += ` · ${W.dayOf(it.day + 1, it.span)}`;
    box.append(h('div.det-when', {}, when));
    if (it.late) box.append(h('div.warn', {}, `${W.late} · ${W.wasOn(fmtDate(it.origin, { day: 'numeric', month: 'long' }))}`));
    if (it.deadline) box.append(h('div.warn', {}, W.deadline));
    if (it.projected) box.append(h('div.muted', {}, `↻ ${W.upcoming}`));

    if (it.kind === 'external') {
      if (det.response === 'pending') box.append(h('div.warn', {}, W.tentative));
      if (det.response === 'tentative') box.append(h('div.warn', {}, W.maybe));
      const acts = [];
      if (det.video_call) acts.push(h('a.btn.primary', { href: det.video_call, target: '_blank', rel: 'noopener noreferrer' }, '▶ ', W.join));
      if (det.link) acts.push(h('a.btn', { href: det.link, target: '_blank', rel: 'noopener noreferrer' }, W.openGoogle));
      if (acts.length) box.append(h('div.actions', {}, acts));
      const rows = [];
      if (det.location) rows.push([W.where, linkify(det.location)]);
      if (rows.length) box.append(h('dl.det-rows', {}, rows.flatMap(([k, v]) => [h('dt', {}, k), h('dd', {}, v)])));
      if (det.attendees && det.attendees.length) {
        box.append(h('div.set-h', {}, `${W.people} · ${det.attendees.length}`));
        const icon = { accepted: '✓', declined: '✕', tentative: '?', needsAction: '·' };
        box.append(h('ul.people', {}, det.attendees.slice(0, 40).map((p) => h('li', { title: W[p.response] || p.response },
          h('span.st', { style: { color: p.response === 'accepted' ? 'var(--ok)' : p.response === 'declined' ? 'var(--overdue)' : 'var(--dim)' } }, icon[p.response] || '·'),
          h('span', {}, p.name || p.email, p.organizer ? h('span.muted', {}, ` · ${W.organizer}`) : null)))));
      }
      if (det.attachments && det.attachments.length) {
        box.append(h('div.set-h', {}, W.files));
        box.append(h('div.actions', {}, det.attachments.map((a) => h('a.btn', { href: a.url, target: '_blank', rel: 'noopener noreferrer' }, '📎 ', a.title || a.url))));
      }
      if (det.description) {
        box.append(h('div.set-h', {}, W.about));
        box.append(h('div.desc', {}, linkify(det.description)));
      }
      box.append(h('p.muted', { style: { fontSize: '13px', marginTop: '18px' } }, W.readOnlyExt));
    } else {
      const rows = [];
      if (det.due && (det.due !== it.origin || !it.deadline)) rows.push([W.due, fmtDate(det.due, { weekday: 'short', day: 'numeric', month: 'long' })]);
      if (det.repeat) rows.push([W.repeat, repeatLabel(det.repeat)]);
      if (det.priority) rows.push([W.priority, h('span', { style: { color: S.theme.priority[det.priority] } }, `${det.priority} · ${W.prio[det.priority] || ''}`)]);
      if (det.tags && det.tags.length) rows.push([W.tags, det.tags.map((t) => '@' + t).join(' ')]);
      if (det.done) rows.push([W.done, det.done_on ? `${W.doneOn} ${fmtDate(det.done_on, { day: 'numeric', month: 'long' })}` : '✓']);
      if (det.subtasks) rows.push([W.subtasks, h('div', {}, `${det.subtasks.done}/${det.subtasks.total}`,
        h('div.progress', {}, h('div', { style: { width: (100 * det.subtasks.done / det.subtasks.total) + '%' } })))]);
      if (rows.length) box.append(h('dl.det-rows', {}, rows.flatMap(([k, v]) => [h('dt', {}, k), h('dd', {}, v)])));
      if (det.notes && det.notes.length) {
        box.append(h('div.set-h', {}, W.notes));
        for (const n of det.notes) {
          const md = h('div.md', { html: n.html });
          for (const a of md.querySelectorAll('a')) { a.target = '_blank'; a.rel = 'noopener noreferrer'; }
          box.append(h('div.note-card', {}, md));
        }
      }
      box.append(h('p.muted', { style: { fontSize: '13px', marginTop: '18px' } }, W.readOnly));
    }
    root.append(box);
  }

  // ── sheets ─────────────────────────────────────────────
  function showSheet() {
    $('#sheet').hidden = false;
    $('#scrim').hidden = false;
    $('#sheet-body').scrollTop = 0;
    $('#sheet-close').focus({ preventScroll: true });
  }
  function closeSheet() {
    $('#sheet').hidden = true;
    $('#scrim').hidden = true;
    S.sheet = null;
  }

  let installPrompt = null;
  addEventListener('beforeinstallprompt', (e) => { e.preventDefault(); installPrompt = e; });

  function openSettings() {
    S.sheet = 'settings';
    const body = $('#sheet-body');
    const cur = S.theme ? S.theme.name : DEFAULT_THEME;
    const themeCard = (t) => h('button', { type: 'button', class: 'theme-card', 'aria-pressed': String(t.name === cur),
      style: { background: t.bg, color: t.fg }, onclick: () => { keep.set('theme', t.name); applyTheme(); render(); openSettings(); } },
      h('div', { style: { fontWeight: 650, fontSize: '14px' } }, t.name),
      h('div.sw', {}, t.palette.slice(0, 6).map((p) => h('i', { style: { background: p } }))));
    const sw = (on, fn) => h('button', { type: 'button', class: 'switch', role: 'switch', 'aria-checked': String(on), onclick: fn });
    body.replaceChildren(...[
      h('h2.det-title', {}, W.settings),
      h('div.set-h', {}, W.theme),
      h('div.themes', {}, S.themes.filter((t) => !t.mine).map(themeCard)),
      S.themes.some((t) => t.mine) ? h('div.set-h', {}, W.myThemes) : null,
      S.themes.some((t) => t.mine) ? h('div.themes', {}, S.themes.filter((t) => t.mine).map(themeCard)) : null,
      h('p.muted', { style: { fontSize: '13px' } }, W.myThemesSub), 
      h('div.opt', {}, h('div', {}, W.fullDay, h('small', {}, W.fullDaySub)), sw(S.full, () => { toggleFull(); openSettings(); })),
      isWide() ? h('div.opt', {}, h('div', {}, W.titles, h('small', {}, W.titlesSub)),
        sw(S.titles, () => { S.titles = !S.titles; keep.set('titles', S.titles); render(); openSettings(); })) : null,
      matchMedia('(display-mode: standalone)').matches ? null : h('div.opt', {}, h('div', {}, W.install, h('small', {}, installPrompt ? '' : W.installIos)),
        installPrompt ? h('button.btn', { type: 'button', onclick: () => installPrompt.prompt() }, W.installBtn) : null),
      h('form', { method: 'post', action: '/logout', style: { marginTop: '22px' } },
        h('button.btn', { type: 'submit', style: { width: '100%', justifyContent: 'center' } }, W.signOut),
        h('p.muted', { style: { fontSize: '13px' } }, W.signOutSub)),
    ].filter(Boolean));
    showSheet();
  }

  function openHelp() {
    S.sheet = 'help';
    const k = (keys, what) => [h('span', {}, keys.map((x) => h('kbd', {}, x)).reduce((a, b) => [a, ' ', b])), h('span', {}, what)];
    $('#sheet-body').replaceChildren(
      h('h2.det-title', {}, W.keys),
      h('p.muted', {}, W.keysSub),
      h('div.keys', {},
        k(['h', 'l'], W.kPrevNext), k(['t'], W.kToday), k(['d', 'w', 'm', 'a'], W.kViews),
        k(['j', 'k'], W.kMove), k(['Enter'], W.kOpen), k(['Esc'], W.kClose), k(['z'], W.kHours),
        k(['r'], W.kRefresh), k([','], W.kSettings), k(['?'], W.kHelp)),
    );
    showSheet();
  }

  // ── gestures & keys ────────────────────────────────────
  function swipe(el, fn) {
    let x0 = null, y0 = 0, t0 = 0;
    el.ontouchstart = (e) => { if (e.touches.length !== 1) return; x0 = e.touches[0].clientX; y0 = e.touches[0].clientY; t0 = Date.now(); };
    el.ontouchend = (e) => {
      if (x0 === null) return;
      const dx = e.changedTouches[0].clientX - x0, dy = e.changedTouches[0].clientY - y0;
      x0 = null;
      if (Math.abs(dx) > 60 && Math.abs(dx) > 1.6 * Math.abs(dy) && Date.now() - t0 < 600) fn(dx < 0 ? 1 : -1);
    };
  }

  // Every item on screen, in order, for j / k.
  function visibleItems() {
    const v = view();
    const days = v === 'day' ? [S.cur] : v === 'month' ? [S.cur] : weekDays();
    return days.flatMap((d) => itemsOf(d).map((it) => ({ it, d })));
  }
  function move(n) {
    const list = visibleItems();
    if (!list.length) return;
    let i = S.sel ? list.findIndex((x) => x.it.key === S.sel.key && x.d === S.sel.date && x.it.start === S.sel.start) : -1;
    i = i < 0 ? (n > 0 ? 0 : list.length - 1) : Math.max(0, Math.min(list.length - 1, i + n));
    const { it, d } = list[i];
    S.sel = { key: it.key, date: d, start: it.start, origin: it.origin };
    S.cur = d;
    render();
    const el = document.querySelector('.is-sel');
    if (el) el.scrollIntoView({ block: 'nearest' });
    if (S.sheet === 'detail') fillDetail($('#sheet-body'));
  }

  addEventListener('keydown', (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (/^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName)) return;
    const k = e.key;
    if (k === 'Escape') { if (!$('#sheet').hidden) closeSheet(); else { S.sel = null; render(); } return; }
    const map = {
      h: () => step(-1), ArrowLeft: () => step(-1), l: () => step(1), ArrowRight: () => step(1),
      t: () => go(localToday()), d: () => setView('day'), w: () => setView('week'), m: () => setView('month'), a: () => setView('agenda'),
      j: () => move(1), ArrowDown: () => move(1), k: () => move(-1), ArrowUp: () => move(-1),
      Enter: () => { if (S.sel) { S.sheet = 'detail'; fillDetail($('#sheet-body')); showSheet(); } },
      o: () => { if (S.sel) { S.sheet = 'detail'; fillDetail($('#sheet-body')); showSheet(); } },
      r: () => { load(true).then(() => toast(W.updated)); }, '?': openHelp, ',': openSettings, z: toggleFull,
    };
    if ((k === 'ArrowDown' || k === 'ArrowUp') && !S.sel && !(view() === 'week' || view() === 'day')) return;
    const fn = map[k];
    if (fn) { e.preventDefault(); fn(); }
  });

  // ── wiring ─────────────────────────────────────────────
  $('#prev').onclick = () => step(-1);
  $('#next').onclick = () => step(1);
  $('#today').onclick = () => go(localToday());
  $('#title').onclick = () => setView(view() === 'month' ? (isWide() ? 'week' : 'agenda') : 'month');
  $('#refresh').onclick = () => load(true).then(() => toast(W.updated));
  $('#help').onclick = openHelp;
  $('#menu').onclick = openSettings;
  $('#scrim').onclick = closeSheet;
  $('#sheet-close').onclick = closeSheet;
  swipe($('#strip'), (dir) => go(add(S.cur, 7 * dir)));
  // Drag the sheet down to close it.
  (() => {
    const sheet = $('#sheet');
    let y0 = null;
    sheet.addEventListener('touchstart', (e) => { if ($('#sheet-body').scrollTop <= 0) y0 = e.touches[0].clientY; }, { passive: true });
    sheet.addEventListener('touchend', (e) => { if (y0 !== null && e.changedTouches[0].clientY - y0 > 90) closeSheet(); y0 = null; });
  })();

  let lastWide = isWide();
  addEventListener('resize', () => { if (isWide() !== lastWide) { lastWide = isWide(); S.scrolled = null; render(); load(); } });

  // Fresh every minute, and on coming back to the tab.
  let lastDay = localToday();
  setInterval(() => {
    if (document.hidden) return;
    if (localToday() !== lastDay) { lastDay = localToday(); if (S.cur === add(lastDay, -1)) S.cur = lastDay; }
    load(true);
  }, 60_000);
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden) { lastDay = localToday(); load(); render(); }
  });

  fetch('/themes.json').then((r) => r.json()).then((list) => {
    S.themes = list.concat(S.themes.filter((t) => !list.some((x) => x.name === t.name)));
    applyTheme();
    render();
  }).catch(() => {});
  render();
  load(true);
  if ('serviceWorker' in navigator) navigator.serviceWorker.register('/sw.js').catch(() => {});
})();
