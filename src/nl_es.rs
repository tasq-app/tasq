//! Spanish for the add dialog, as a vocabulary over the English parser.
//!
//! What's typed is read into a *shadow*: the same words, with each Spanish
//! phrase tasq knows replaced by the English one the parser understands
//! ("a las 6 de la tarde" → `at 6pm`, "la semana que viene" → `next week`,
//! "del 16 al 17 de nov" → `from 16 to 17 nov`). The parser runs on the
//! shadow; every token remembers the words it came from, so the phrases it
//! finds are lit up — and left out of the title — in what was typed.
//!
//! Another language is another table like [`RULES`].

/// A rule's pattern piece: a word (accents and case folded), or a slot.
/// Slots: `#` a number (digits, a clock like `18:30`, or a number word),
/// `@` a weekday, `!` a month, `&` a unit of time.
type Rule = (&'static [&'static str], &'static [&'static str]);

/// Spanish phrases and what they mean in the English the parser reads.
/// Longer phrases are tried first; in the replacement a slot stands for
/// what it matched (`#pm` → `6pm`).
const RULES: &[Rule] = &[
    // Time of day.
    (&["a", "las"], &["at"]),
    (&["a", "la"], &["at"]),
    (&["#", "de", "la", "tarde"], &["#pm"]),
    (&["#", "de", "la", "noche"], &["#pm"]),
    (&["#", "de", "la", "manana"], &["#am"]),
    (&["#", "de", "la", "madrugada"], &["#am"]),
    (&["#", "y", "media"], &["#:30"]),
    (&["#", "y", "cuarto"], &["#:15"]),
    (&["mediodia"], &["noon"]),
    (&["medianoche"], &["midnight"]),
    (&["esta", "noche"], &["tonight"]),
    // "Por la mañana" is a part of the day, not tomorrow.
    (&["por", "la", "manana"], &["in", "the", "morning"]),
    // Days.
    (&["pasado", "manana"], &["in", "2", "days"]),
    (&["hoy"], &["today"]),
    (&["manana"], &["tomorrow"]),
    (&["ayer"], &["yesterday"]),
    (&["el", "proximo", "@"], &["next", "@"]),
    (&["el", "@", "que", "viene"], &["next", "@"]),
    (&["proximo", "@"], &["next", "@"]),
    (&["este", "@"], &["this", "@"]),
    (&["todos", "los", "@"], &["every", "@"]),
    (&["los", "@"], &["every", "@"]),
    (&["el", "@"], &["on", "@"]),
    // "El día 15 de noviembre", "el 15 nov", "el lunes 16", "el 15 de
    // noviembre de 2027".
    (&["el", "dia", "#", "de", "!", "de", "#"], &["!", "#", "#"]),
    (&["el", "#", "de", "!", "de", "#"], &["!", "#", "#"]),
    (&["#", "de", "!", "de", "#"], &["!", "#", "#"]),
    (&["el", "dia", "#", "de", "!"], &["!", "#"]),
    (&["dia", "#", "de", "!"], &["!", "#"]),
    (&["el", "dia", "#", "!"], &["!", "#"]),
    (&["el", "#", "!"], &["!", "#"]),
    (&["el", "dia", "#"], &["the", "#"]),
    (&["el", "!", "#", "#"], &["!", "#", "#"]),
    (&["el", "!", "#"], &["!", "#"]),
    (&["el", "@", "#"], &["the", "#"]),
    (&["el", "@", "dia", "#"], &["the", "#"]),
    // Repeats with a number: "cada dos lunes", "cada 3 martes y jueves",
    // "cada día 15" (of each month), "una vez a la semana".
    (&["cada", "#", "@"], &["every", "#", "@"]),
    (
        &["cada", "dia", "#", "del", "mes"],
        &["the", "#", "every", "month"],
    ),
    (
        &["cada", "dia", "#", "de", "cada", "mes"],
        &["the", "#", "every", "month"],
    ),
    (
        &["el", "#", "de", "cada", "mes"],
        &["the", "#", "every", "month"],
    ),
    (
        &["el", "dia", "#", "de", "cada", "mes"],
        &["the", "#", "every", "month"],
    ),
    (
        &["todos", "los", "#", "de", "mes"],
        &["the", "#", "every", "month"],
    ),
    (&["cada", "dia", "#"], &["the", "#", "every", "month"]),
    (&["una", "vez", "a", "la", "semana"], &["every", "week"]),
    (&["una", "vez", "por", "semana"], &["every", "week"]),
    (&["una", "vez", "al", "mes"], &["every", "month"]),
    (&["una", "vez", "al", "dia"], &["every", "day"]),
    (&["una", "vez", "al", "ano"], &["every", "year"]),
    (&["a", "diario"], &["daily"]),
    (
        &["todos", "los", "dias", "de", "lunes", "a", "viernes"],
        &["every", "weekday"],
    ),
    (
        &["cada", "dia", "de", "lunes", "a", "viernes"],
        &["every", "weekday"],
    ),
    (&["de", "lunes", "a", "viernes"], &["every", "weekday"]),
    (&["de", "@", "a", "@"], &["from", "@", "to", "@"]),
    // "Lunes y miércoles" with no date before: every monday and wednesday.
    (&["@", "y", "@"], &["every", "@", "and", "@"]),
    // How long a repeat lasts: "durante 5 semanas".
    (&["durante", "#", "&"], &["for", "#", "&"]),
    (&["el", "#", "de", "!"], &["!", "#"]),
    (&["#", "de", "!"], &["#", "!"]),
    (&["el", "#"], &["the", "#"]),
    // Stretches of time.
    (&["la", "semana", "que", "viene"], &["next", "week"]),
    (&["la", "proxima", "semana"], &["next", "week"]),
    (&["la", "semana", "proxima"], &["next", "week"]),
    (&["semana", "que", "viene"], &["next", "week"]),
    (&["proxima", "semana"], &["next", "week"]),
    (&["el", "mes", "que", "viene"], &["next", "month"]),
    (&["el", "proximo", "mes"], &["next", "month"]),
    (&["el", "mes", "proximo"], &["next", "month"]),
    (&["mes", "que", "viene"], &["next", "month"]),
    (&["proximo", "mes"], &["next", "month"]),
    (&["el", "finde", "que", "viene"], &["next", "weekend"]),
    (&["el", "proximo", "finde"], &["next", "weekend"]),
    (
        &["el", "proximo", "fin", "de", "semana"],
        &["next", "weekend"],
    ),
    (
        &["el", "fin", "de", "semana", "que", "viene"],
        &["next", "weekend"],
    ),
    (&["este", "fin", "de", "semana"], &["this", "weekend"]),
    (&["este", "finde"], &["this", "weekend"]),
    (&["el", "fin", "de", "semana"], &["the", "weekend"]),
    (&["el", "finde"], &["the", "weekend"]),
    (&["fin", "de", "semana"], &["weekend"]),
    (&["finde"], &["weekend"]),
    (&["esta", "semana"], &["this", "week"]),
    (&["este", "mes"], &["this", "month"]),
    (
        &["el", "resto", "de", "la", "semana"],
        &["the", "rest", "of", "the", "week"],
    ),
    (
        &["el", "resto", "del", "mes"],
        &["the", "rest", "of", "the", "month"],
    ),
    (
        &["durante", "la", "proxima", "semana"],
        &["for", "the", "next", "week"],
    ),
    (
        &["en", "la", "proxima", "semana"],
        &["within", "the", "next", "week"],
    ),
    (
        &["durante", "el", "proximo", "mes"],
        &["for", "the", "next", "month"],
    ),
    (
        &["durante", "los", "proximos", "#", "&"],
        &["for", "the", "next", "#", "&"],
    ),
    (
        &["en", "los", "proximos", "#", "&"],
        &["within", "the", "next", "#", "&"],
    ),
    (&["los", "proximos", "#", "&"], &["the", "next", "#", "&"]),
    (&["dentro", "de"], &["in"]),
    // Ranges: "del 16 al 17", "desde el lunes hasta el viernes".
    (&["del"], &["from"]),
    (&["al"], &["to"]),
    (&["desde", "el"], &["from"]),
    (&["desde"], &["from"]),
    (&["hasta", "el"], &["until"]),
    (&["hasta"], &["until"]),
    (&["entre", "semana"], &["every", "weekday"]),
    (&["entre"], &["between"]),
    (&["y"], &["and"]),
    // Repeats.
    (
        &["todos", "los", "dias", "laborables"],
        &["every", "weekday"],
    ),
    (&["cada", "dia", "laborable"], &["every", "weekday"]),
    (&["todos", "los", "dias"], &["every", "day"]),
    (&["todas", "las", "semanas"], &["every", "week"]),
    (&["todos", "los", "meses"], &["every", "month"]),
    (&["todos", "los", "anos"], &["every", "year"]),
    (&["cada"], &["every"]),
    (&["diario"], &["daily"]),
    (&["diariamente"], &["daily"]),
    (&["semanal"], &["weekly"]),
    (&["semanalmente"], &["weekly"]),
    (&["quincenal"], &["biweekly"]),
    (&["mensual"], &["monthly"]),
    (&["mensualmente"], &["monthly"]),
    (&["anual"], &["yearly"]),
    (&["anualmente"], &["yearly"]),
    (&["veces"], &["times"]),
    (&["vez"], &["times"]),
    // How long, and reminders.
    (&["una", "hora", "y", "media"], &["1.5", "hours"]),
    (&["#", "horas", "y", "media"], &["#.5", "hours"]),
    (&["media", "hora"], &["half", "an", "hour"]),
    (&["durante"], &["for"]),
    (&["por", "#"], &["for", "#"]),
    (&["un", "&"], &["1", "&"]),
    (&["una", "&"], &["1", "&"]),
    (&["recuerdamelo"], &["remind", "me"]),
    (&["recuerdame"], &["remind", "me"]),
    (&["avisame"], &["remind", "me"]),
    (&["recordatorio"], &["remind", "me"]),
    (&["antes", "del"], &["by"]),
    (&["antes", "de", "el"], &["by"]),
    (&["antes"], &["before"]),
    // Deadlines.
    (&["para", "el"], &["by"]),
    (&["como", "tarde", "el"], &["by"]),
    (&["fecha", "limite"], &["deadline"]),
    (&["vence", "el"], &["due"]),
    (&["vence"], &["due"]),
    (&["para"], &["on"]),
    // Priority.
    (&["prioridad", "alta"], &["high", "priority"]),
    (&["alta", "prioridad"], &["high", "priority"]),
    (&["muy", "importante"], &["high", "priority"]),
    (&["urgente"], &["high", "priority"]),
    (&["prioridad", "media"], &["medium", "priority"]),
    (&["media", "prioridad"], &["medium", "priority"]),
    (&["prioridad", "baja"], &["low", "priority"]),
    (&["baja", "prioridad"], &["low", "priority"]),
    // Events, spaces.
    (&["evento"], &["event"]),
    (&["en"], &["in"]),
];

/// Accents and case away: "Miércoles" → `miercoles`, "mañana" → `manana`.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

fn weekday(w: &str) -> Option<&'static str> {
    Some(match w {
        "lunes" | "lun" => "monday",
        "martes" => "tuesday",
        "miercoles" | "mie" | "mier" => "wednesday",
        "jueves" | "jue" => "thursday",
        "viernes" | "vie" => "friday",
        "sabado" | "sabados" | "sab" => "saturday",
        "domingo" | "domingos" | "dom" => "sunday",
        _ => return None,
    })
}

fn month(w: &str) -> Option<&'static str> {
    Some(match w {
        "enero" | "ene" => "jan",
        "febrero" | "feb" => "feb",
        "marzo" | "mar" => "mar",
        "abril" | "abr" => "apr",
        "mayo" | "may" => "may",
        "junio" | "jun" => "jun",
        "julio" | "jul" => "jul",
        "agosto" | "ago" => "aug",
        "septiembre" | "setiembre" | "sep" | "sept" => "sep",
        "octubre" | "oct" => "oct",
        "noviembre" | "nov" => "nov",
        "diciembre" | "dic" => "dec",
        _ => return None,
    })
}

fn unit(w: &str) -> Option<&'static str> {
    Some(match w {
        "dia" => "day",
        "dias" => "days",
        "semana" => "week",
        "semanas" => "weeks",
        "mes" => "month",
        "meses" => "months",
        "ano" => "year",
        "anos" => "years",
        "hora" => "hour",
        "horas" => "hours",
        "minuto" => "minute",
        "minutos" => "minutes",
        "min" | "mins" => "min",
        "h" => "h",
        _ => return None,
    })
}

fn number(w: &str) -> Option<String> {
    if w.chars().any(|c| c.is_ascii_digit())
        && w.chars()
            .all(|c| c.is_ascii_digit() || c == ':' || c == '.')
    {
        return Some(w.to_string());
    }
    let n = match w {
        "dos" => 2,
        "tres" => 3,
        "cuatro" => 4,
        "cinco" => 5,
        "seis" => 6,
        "siete" => 7,
        "ocho" => 8,
        "nueve" => 9,
        "diez" => 10,
        "once" => 11,
        "doce" => 12,
        "quince" => 15,
        "veinte" => 20,
        "treinta" => 30,
        "cuarenta" => 40,
        "cuarenta y cinco" => 45,
        _ => return None,
    };
    Some(n.to_string())
}

/// Words that keep their meaning alone: units, weekdays, months, numbers.
fn single(w: &str) -> Option<String> {
    weekday(w)
        .or_else(|| month(w))
        .or_else(|| unit(w))
        .map(str::to_string)
        .or_else(|| number(w).filter(|_| !w.starts_with(|c: char| c.is_ascii_digit())))
}

/// "3horas" → `3 hours`, "1.5hours" → `1.5 hours`, "15nov" → `15 nov`,
/// "nov15" → `nov 15`: a number stuck to a unit spelled out or to a month.
/// Short units ("3h", "90m") are left to the parser, which reads them.
fn glued(w: &str) -> Option<Vec<String>> {
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    // Month first: "nov15".
    if let Some(at) = w.find(|c: char| c.is_ascii_digit()) {
        let (m, d) = w.split_at(at);
        if let Some(m) = month(m)
            && digits(d)
        {
            return Some(vec![m.to_string(), d.to_string()]);
        }
    }
    let split = w.find(|c: char| c.is_ascii_alphabetic())?;
    let (n, u) = w.split_at(split);
    if n.is_empty()
        || !n
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return None;
    }
    if let Some(m) = month(u).filter(|_| digits(n)) {
        return Some(vec![n.to_string(), m.to_string()]);
    }
    let unit = match u {
        "hours" | "hour" | "hrs" | "hr" | "minutes" | "minute" | "mins" | "days" | "day"
        | "weeks" | "week" | "months" | "month" => u,
        u if u.len() >= 3 => self::unit(u)?,
        _ => return None,
    };
    Some(vec![n.replace(',', "."), unit.to_string()])
}

/// "15/11" → `15 nov`, "15/11/2027" → `nov 15 2027`: day first, the
/// Spanish way.
fn slash_date(w: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = w.split('/').collect();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let (d, m): (u32, u32) = (parts[0].parse().ok()?, parts[1].parse().ok()?);
    if !(1..=31).contains(&d) || !(1..=12).contains(&m) {
        return None;
    }
    const NAMES: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let month = NAMES[m as usize - 1].to_string();
    match parts.get(2) {
        Some(y) => {
            let y: u32 = y.parse().ok()?;
            let y = if y < 100 { 2000 + y } else { y };
            Some(vec![month, d.to_string(), y.to_string()])
        }
        None => Some(vec![d.to_string(), month]),
    }
}

/// The text as the parser reads it, and where each of its tokens came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shadow {
    pub text: String,
    /// For each token: its range in `text` and the range in what was typed.
    pub map: Vec<((usize, usize), (usize, usize))>,
}

impl Shadow {
    /// Whether nothing was translated.
    pub fn is_identity(&self, typed: &str) -> bool {
        self.text == typed
    }

    /// The range in what was typed that the shadow range `start..end`
    /// covers.
    pub fn to_typed(&self, start: usize, end: usize) -> (usize, usize) {
        let mut out: Option<(usize, usize)> = None;
        for &((s, e), (os, oe)) in &self.map {
            if s < end && e > start {
                out = Some(match out {
                    Some((a, b)) => (a.min(os), b.max(oe)),
                    None => (os, oe),
                });
            }
        }
        out.unwrap_or((0, 0))
    }
}

/// The words of `s` with their byte ranges.
fn words(s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if let Some(st) = start.take() {
                out.push((st, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        out.push((st, s.len()));
    }
    out
}

/// Trailing punctuation a word keeps when it's translated ("lunes," →
/// `monday,`).
fn trailing_punct(w: &str) -> &str {
    let trimmed = w.trim_end_matches([',', '.', ';', ':', '!', '?']);
    &w[trimmed.len()..]
}

/// Read `typed` into the [`Shadow`] the parser works on.
pub fn shadow(typed: &str) -> Shadow {
    /// One piece of what was typed: a word, or part of one run together
    /// ("15nov" is `15` and `nov`), all from the same typed range.
    struct Piece<'a> {
        from: (usize, usize),
        raw: std::borrow::Cow<'a, str>,
        folded: String,
        punct: &'a str,
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for (s, e) in words(typed) {
        let raw = &typed[s..e];
        let punct = trailing_punct(raw);
        let folded = fold(&raw[..raw.len() - punct.len()]);
        let split = (!raw.starts_with(['+', '@']))
            .then(|| glued(&folded).or_else(|| slash_date(&folded)))
            .flatten();
        match split {
            Some(toks) => {
                let n = toks.len();
                for (k, t) in toks.into_iter().enumerate() {
                    pieces.push(Piece {
                        from: (s, e),
                        raw: t.clone().into(),
                        folded: t,
                        punct: if k + 1 == n { punct } else { "" },
                    });
                }
            }
            None => pieces.push(Piece {
                from: (s, e),
                raw: raw.into(),
                folded,
                punct,
            }),
        }
    }
    let mut rules: Vec<&Rule> = RULES.iter().collect();
    rules.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));

    let mut text = String::new();
    let mut map = Vec::new();
    let mut push = |text: &mut String, tok: &str, from: (usize, usize)| {
        if !text.is_empty() {
            text.push(' ');
        }
        let s = text.len();
        text.push_str(tok);
        map.push(((s, text.len()), from));
    };
    let mut i = 0;
    while i < pieces.len() {
        // A sigiled tag is never translated: `+Uni`, `@casa`.
        if pieces[i].raw.starts_with(['+', '@']) {
            push(&mut text, &pieces[i].raw, pieces[i].from);
            i += 1;
            continue;
        }
        let mut done = false;
        for (pat, rep) in &rules {
            if i + pat.len() > pieces.len() {
                continue;
            }
            // "Lunes y miércoles" on its own is every monday and wednesday;
            // inside a list ("cada lunes, miércoles y viernes") it's left
            // to the list.
            if pat.first() == Some(&"@")
                && i > 0
                && (weekday(&pieces[i - 1].folded).is_some()
                    || matches!(
                        pieces[i - 1].folded.as_str(),
                        "cada" | "los" | "todos" | "el" | "de" | "entre" | "y"
                    ))
            {
                continue;
            }
            let mut slots: Vec<(char, String)> = Vec::new();
            let fits = pat.iter().enumerate().all(|(k, p)| {
                let piece = &pieces[i + k];
                // Only the last word may carry punctuation.
                if k + 1 < pat.len() && !piece.punct.is_empty() {
                    return false;
                }
                let w = piece.folded.as_str();
                let got = match *p {
                    "#" => number(w),
                    "@" => weekday(w).map(str::to_string),
                    "!" => month(w).map(str::to_string),
                    "&" => unit(w).map(str::to_string),
                    lit => (lit == w).then(String::new),
                };
                match (got, p.chars().next()) {
                    (Some(v), Some(c @ ('#' | '@' | '!' | '&'))) if p.len() == 1 => {
                        slots.push((c, v));
                        true
                    }
                    (Some(_), _) => true,
                    (None, _) => false,
                }
            });
            if !fits {
                continue;
            }
            let last = &pieces[i + pat.len() - 1];
            let from = (pieces[i].from.0, last.from.1);
            let punct = last.punct;
            // Each slot kind can be used twice ("de @ a @"): fill in order.
            let mut used: Vec<usize> = Vec::new();
            for (n, r) in rep.iter().enumerate() {
                let mut tok = String::new();
                for ch in r.chars() {
                    if matches!(ch, '#' | '@' | '!' | '&') {
                        let pick = slots
                            .iter()
                            .enumerate()
                            .find(|(k, (c, _))| *c == ch && !used.contains(k))
                            .or_else(|| {
                                slots.iter().enumerate().rev().find(|(_, (c, _))| *c == ch)
                            });
                        if let Some((k, (_, v))) = pick {
                            used.push(k);
                            tok.push_str(v);
                            continue;
                        }
                    }
                    tok.push(ch);
                }
                if n + 1 == rep.len() {
                    tok.push_str(punct);
                }
                push(&mut text, &tok, from);
            }
            i += pat.len();
            done = true;
            break;
        }
        if done {
            continue;
        }
        let p = &pieces[i];
        let tok = match single(&p.folded) {
            Some(t) => format!("{t}{}", p.punct),
            None => p.raw.to_string(),
        };
        push(&mut text, &tok, p.from);
        i += 1;
    }
    Shadow { text, map }
}

/// What's left of `typed` for the title once the `spans` (ranges in what
/// was typed) are taken out: the words they don't cover, without
/// connectors at either end.
pub fn body(typed: &str, spans: &[(usize, usize)]) -> String {
    let mut kept: Vec<&str> = words(typed)
        .into_iter()
        .filter(|&(s, e)| !spans.iter().any(|&(a, b)| a <= s && e <= b))
        .map(|(s, e)| &typed[s..e])
        .collect();
    let connector = |t: &str| {
        matches!(
            fold(t.trim_matches([',', '.', ';', ':', '!', '?'])).as_str(),
            "y" | "o" | "pero" | "que" | "and" | "or" | "but" | ""
        )
    };
    while kept.first().is_some_and(|t| connector(t)) {
        kept.remove(0);
    }
    while kept.last().is_some_and(|t| connector(t)) {
        kept.pop();
    }
    kept.join(" ")
        .trim_matches(|c: char| matches!(c, ',' | '.' | ';' | ':') || c.is_whitespace())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_reads_as_the_english_the_parser_knows() {
        for (typed, read) in [
            (
                "comprar pan mañana a las 6 de la tarde",
                "comprar pan tomorrow at 6pm",
            ),
            (
                "gimnasio cada lunes, miércoles y viernes",
                "gimnasio every monday, wednesday and friday",
            ),
            ("viaje del 16 al 17 de nov", "viaje from 16 to 17 nov"),
            ("examen el 15 de mayo", "examen may 15"),
            (
                "llamar a mamá la semana que viene",
                "llamar a mamá next week",
            ),
            ("pagar +Casa este finde", "pagar +Casa this weekend"),
            ("pan durante 3horas", "pan for 3 hours"),
            ("pan for 2hours", "pan for 2 hours"),
            ("pan el 15nov", "pan nov 15"),
            ("pan el 15/11/2027", "pan nov 15 2027"),
        ] {
            assert_eq!(shadow(typed).text, read, "{typed}");
        }
        let english = "buy milk tomorrow at 6pm";
        assert!(shadow(english).is_identity(english));
    }

    #[test]
    fn a_shadow_range_maps_back_to_what_was_typed() {
        let typed = "pan a las 6 de la tarde";
        let sh = shadow(typed);
        let at = sh.text.find("at 6pm").unwrap_or(0);
        let (s, e) = sh.to_typed(at, at + "at 6pm".len());
        assert_eq!(&typed[s..e], "a las 6 de la tarde");
        assert_eq!(body(typed, &[(s, e)]), "pan");
    }
}
