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
    let ws = words(typed);
    let folded: Vec<String> = ws
        .iter()
        .map(|&(s, e)| fold(typed[s..e].trim_end_matches([',', '.', ';', ':', '!', '?'])))
        .collect();
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
    while i < ws.len() {
        // A sigiled tag is never translated: `+Uni`, `@casa`.
        let raw = &typed[ws[i].0..ws[i].1];
        if raw.starts_with(['+', '@']) {
            push(&mut text, raw, ws[i]);
            i += 1;
            continue;
        }
        let mut done = false;
        for (pat, rep) in &rules {
            if i + pat.len() > ws.len() {
                continue;
            }
            let mut slots: Vec<(char, String)> = Vec::new();
            let fits = pat.iter().enumerate().all(|(k, p)| {
                let w = folded[i + k].as_str();
                // Only the last word may carry punctuation.
                if k + 1 < pat.len() && !trailing_punct(&typed[ws[i + k].0..ws[i + k].1]).is_empty()
                {
                    return false;
                }
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
            let from = (ws[i].0, ws[i + pat.len() - 1].1);
            let last = &typed[ws[i + pat.len() - 1].0..ws[i + pat.len() - 1].1];
            let punct = trailing_punct(last);
            for (n, r) in rep.iter().enumerate() {
                let mut tok = (*r).to_string();
                for (c, v) in &slots {
                    tok = tok.replace(*c, v);
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
        let tok = match single(&folded[i]) {
            Some(t) => format!("{t}{}", trailing_punct(raw)),
            None => raw.to_string(),
        };
        push(&mut text, &tok, ws[i]);
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
