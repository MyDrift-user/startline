//! The calculator in search: sums ("12 * (3 + 4)", "15% of 80" is "80 * 15%"), conversions between units of length,
//! weight, temperature, volume, data and time ("5 km in mi", "70 f to c", "2 gib in mb"), and dates ("today + 30
//! days"). Nothing here needs the internet; there are no currencies.

use crate::i18n::{Lang, tr, tx};
use crate::m365::calendar::Day;

/// What a search worked out to.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// What's shown: "6,2137 mi" or "= 84".
    pub shown: String,
    /// What Enter copies: the number or date alone.
    pub copy: String,
    /// What was worked out, in words: "Calculator", "Length", "Date".
    pub kind: &'static str,
}

/// Works the search out, if it's a sum, a conversion or a date. Plain numbers and words aren't.
pub fn answer(query: &str) -> Option<Answer> {
    let text = query.trim();
    if text.is_empty() || text.len() > 200 {
        return None;
    }
    date(text).or_else(|| convert(text)).or_else(|| sum(text))
}

// ---- Numbers ----

/// A number the way the language shown writes it, with at most ten significant digits and no trailing zeros.
pub fn show_number(value: f64) -> String {
    show_number_in(crate::i18n::current(), value)
}

fn show_number_in(lang: Lang, value: f64) -> String {
    let (decimal, thousands) = if lang == Lang::En { ('.', ',') } else { (',', '.') };
    let magnitude = if value == 0.0 { 0 } else { value.abs().log10().floor() as i32 };
    if !(-6..15).contains(&magnitude) {
        let text = format!("{value:.6e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{}e{exponent}", mantissa.replace('.', &decimal.to_string()));
    }
    let places = (9 - magnitude).clamp(0, 12) as usize;
    let fixed = format!("{value:.places$}");
    let fixed = if fixed.contains('.') { fixed.trim_end_matches('0').trim_end_matches('.').to_string() } else { fixed };
    let fixed = if fixed == "-0" { "0".to_string() } else { fixed };
    let (whole, part) = fixed.split_once('.').map_or((fixed.as_str(), None), |(w, p)| (w, Some(p)));
    let (sign, digits) = whole.strip_prefix('-').map_or(("", whole), |d| ("-", d));
    let mut grouped = String::new();
    for (at, c) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at) % 3 == 0 && digits.len() > 4 {
            grouped.push(thousands);
        }
        grouped.push(c);
    }
    match part {
        Some(part) => format!("{sign}{grouped}{decimal}{part}"),
        None => format!("{sign}{grouped}"),
    }
}

/// The plain number, for copying: no thousands separators, the language's decimal mark.
fn copy_number(value: f64) -> String {
    copy_number_in(crate::i18n::current(), value)
}

fn copy_number_in(lang: Lang, value: f64) -> String {
    show_number_in(lang, value).replace(if lang == Lang::En { ',' } else { '.' }, "")
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Word(String),
    Op(char),
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c.is_whitespace() {
            at += 1;
        } else if c.is_ascii_digit() || (c == '.' || c == ',') && chars.get(at + 1).is_some_and(|n| n.is_ascii_digit()) {
            let start = at;
            while at < chars.len() && (chars[at].is_ascii_digit() || chars[at] == '.' || chars[at] == ',' || chars[at] == '\'' || chars[at] == '_') {
                at += 1;
            }
            out.push(Token::Number(number(&chars[start..at].iter().collect::<String>())?));
        } else if c.is_alphabetic() || c == '°' || c == '"' {
            let start = at;
            while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '°' || chars[at] == '"') {
                at += 1;
            }
            out.push(Token::Word(chars[start..at].iter().collect::<String>().to_lowercase()));
        } else if "+-*/^%()x×÷=".contains(c) || c == '\u{2212}' {
            out.push(Token::Op(match c {
                '×' => '*',
                '÷' => '/',
                '\u{2212}' => '-',
                other => other,
            }));
            at += 1;
        } else {
            return None;
        }
    }
    Some(out)
}

/// A number as typed: "1.5" and "1,5" are both one and a half; "1,000.5", "1.000,5" and "1'000" keep their thousands.
fn number(text: &str) -> Option<f64> {
    let text: String = text.chars().filter(|c| *c != '\'' && *c != '_').collect();
    let (dots, commas) = (text.matches('.').count(), text.matches(',').count());
    let plain = match (dots, commas) {
        (0, 0) => text,
        (_, 0) if dots > 1 => text.replace('.', ""),
        (0, _) if commas > 1 => text.replace(',', ""),
        (0, 1) => text.replace(',', "."),
        (1, 0) => text,
        // Both: whichever comes last is the decimal mark.
        _ if text.rfind('.') > text.rfind(',') => text.replace(',', ""),
        _ => text.replace('.', "").replace(',', "."),
    };
    plain.parse().ok()
}

/// Reads a sum: + - * / ^, a % after a number, brackets, and a few functions and constants.
struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, op: char) -> bool {
        if self.peek() == Some(&Token::Op(op)) || (op == '*' && self.peek() == Some(&Token::Word("x".into()))) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        loop {
            if self.eat('+') {
                value += self.product()?;
            } else if self.eat('-') {
                value -= self.product()?;
            } else {
                return Some(value);
            }
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut value = self.unary()?;
        loop {
            if self.eat('*') {
                value *= self.unary()?;
            } else if self.eat('/') {
                value /= self.unary()?;
            } else if matches!(self.peek(), Some(Token::Op('(')) | Some(Token::Number(_))) || matches!(self.peek(), Some(Token::Word(w)) if function(w).is_some() || constant(w).is_some()) {
                // "2(3 + 4)" and "2pi" multiply.
                value *= self.unary()?;
            } else {
                return Some(value);
            }
        }
    }

    /// A minus binds looser than a power: -2^2 is -4.
    fn unary(&mut self) -> Option<f64> {
        if self.eat('-') {
            return Some(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Option<f64> {
        let mut base = self.atom()?;
        while self.eat('%') {
            base /= 100.0;
        }
        if self.eat('^') {
            // Right to left: 2^3^2 is 2^9.
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        match self.peek()?.clone() {
            Token::Number(n) => {
                self.at += 1;
                Some(n)
            }
            Token::Op('(') => {
                self.at += 1;
                let value = self.sum()?;
                self.eat(')');
                Some(value)
            }
            Token::Word(word) => {
                self.at += 1;
                if let Some(value) = constant(&word) {
                    return Some(value);
                }
                let f = function(&word)?;
                Some(f(self.unary()?))
            }
            Token::Op(_) => None,
        }
    }
}

fn constant(word: &str) -> Option<f64> {
    match word {
        "pi" | "π" => Some(std::f64::consts::PI),
        "e" => Some(std::f64::consts::E),
        _ => None,
    }
}

fn function(word: &str) -> Option<fn(f64) -> f64> {
    Some(match word {
        "sqrt" | "wurzel" | "radice" => f64::sqrt,
        "sin" => |x: f64| x.to_radians().sin(),
        "cos" => |x: f64| x.to_radians().cos(),
        "tan" => |x: f64| x.to_radians().tan(),
        "ln" => f64::ln,
        "log" => f64::log10,
        "abs" => f64::abs,
        "round" => f64::round,
        _ => return None,
    })
}

/// A sum, worked out. It needs an operator or a function: a number alone is a search.
fn sum(text: &str) -> Option<Answer> {
    // "15% of 80", "15 % von 80", "15% di 80".
    let text = ["of", "von", "di"].iter().fold(text.to_string(), |t, word| t.replace(&format!("% {word} "), "% * "));
    let tokens = tokenize(text.trim_end_matches('=').trim())?;
    let operators = tokens.iter().filter(|t| matches!(t, Token::Op(c) if *c != '(' && *c != ')') || matches!(t, Token::Word(w) if w == "x")).count();
    let functions = tokens.iter().filter(|t| matches!(t, Token::Word(w) if function(w).is_some() || constant(w).is_some())).count();
    let numbers = tokens.iter().filter(|t| matches!(t, Token::Number(_))).count();
    // Words other than functions, constants and "x" for times mean it's not a sum.
    if tokens.iter().any(|t| matches!(t, Token::Word(w) if function(w).is_none() && constant(w).is_none() && w != "x")) {
        return None;
    }
    if operators + functions == 0 || numbers + functions == 0 {
        return None;
    }
    // "-5" alone is a number, not a sum.
    if numbers == 1 && functions == 0 && operators == 1 && tokens.first() == Some(&Token::Op('-')) {
        return None;
    }
    let mut parser = Parser { tokens: &tokens, at: 0 };
    let value = parser.sum()?;
    if parser.at != tokens.len() || !value.is_finite() {
        return None;
    }
    Some(Answer { shown: format!("= {}", show_number(value)), copy: copy_number(value), kind: tx("Calculator") })
}

// ---- Units ----

#[derive(Clone, Copy, PartialEq, Debug)]
enum Measure {
    Length,
    Weight,
    Temperature,
    Volume,
    Data,
    Time,
}

impl Measure {
    fn label(self) -> &'static str {
        match self {
            Measure::Length => tx("Length"),
            Measure::Weight => tx("Weight"),
            Measure::Temperature => tx("Temperature"),
            Measure::Volume => tx("Capacity"),
            Measure::Data => tx("Data size"),
            Measure::Time => tx("Time"),
        }
    }
}

/// Each unit: the names it's typed as, the one it's shown as, what it measures, and how many base units it is
/// (metres, kilograms, litres, bytes, seconds; temperatures are worked out apart).
const UNITS: &[(&[&str], &str, Measure, f64)] = &[
    (&["mm", "millimeter", "millimeters", "millimetre", "millimetres", "millimetri"], "mm", Measure::Length, 0.001), // i18n-ignore: unit names
    (&["cm", "centimeter", "centimeters", "centimetre", "centimetres", "zentimeter", "centimetri"], "cm", Measure::Length, 0.01), // i18n-ignore: unit names
    (&["m", "meter", "meters", "metre", "metres", "metri", "metro"], "m", Measure::Length, 1.0), // i18n-ignore: unit names
    (&["km", "kilometer", "kilometers", "kilometre", "kilometres", "chilometri", "chilometro"], "km", Measure::Length, 1000.0), // i18n-ignore: unit names
    (&["in", "inch", "inches", "\"", "zoll", "pollici", "pollice"], "in", Measure::Length, 0.0254), // i18n-ignore: unit names
    (&["ft", "foot", "feet", "fuss", "fuß", "piedi", "piede"], "ft", Measure::Length, 0.3048), // i18n-ignore: unit names
    (&["yd", "yard", "yards"], "yd", Measure::Length, 0.9144), // i18n-ignore: unit names
    (&["mi", "mile", "miles", "meile", "meilen", "miglia", "miglio"], "mi", Measure::Length, 1609.344), // i18n-ignore: unit names
    (&["nmi", "seemeile", "seemeilen"], "nmi", Measure::Length, 1852.0), // i18n-ignore: unit names
    (&["mg", "milligram", "milligrams", "milligramm"], "mg", Measure::Weight, 0.000001), // i18n-ignore: unit names
    (&["g", "gram", "grams", "gramm", "grammi", "grammo"], "g", Measure::Weight, 0.001), // i18n-ignore: unit names
    (&["kg", "kilogram", "kilograms", "kilogramm", "kilo", "chili", "chilo"], "kg", Measure::Weight, 1.0), // i18n-ignore: unit names
    (&["t", "tonne", "tonnes", "ton", "tons", "tonnen", "tonnellate"], "t", Measure::Weight, 1000.0), // i18n-ignore: unit names
    (&["oz", "ounce", "ounces", "unze", "unzen", "once", "oncia"], "oz", Measure::Weight, 0.028349523125), // i18n-ignore: unit names
    (&["lb", "lbs", "pound", "pounds", "pfund", "libbre", "libbra"], "lb", Measure::Weight, 0.45359237), // i18n-ignore: unit names
    (&["st", "stone", "stones"], "st", Measure::Weight, 6.35029318), // i18n-ignore: unit names
    (&["c", "°c", "celsius", "degc"], "°C", Measure::Temperature, 0.0), // i18n-ignore: unit names
    (&["f", "°f", "fahrenheit", "degf"], "°F", Measure::Temperature, 1.0), // i18n-ignore: unit names
    (&["k", "kelvin"], "K", Measure::Temperature, 2.0), // i18n-ignore: unit names
    (&["ml", "milliliter", "milliliters", "millilitre", "millilitres", "millilitri"], "ml", Measure::Volume, 0.001), // i18n-ignore: unit names
    (&["cl", "centiliter", "centilitre"], "cl", Measure::Volume, 0.01), // i18n-ignore: unit names
    (&["dl", "deciliter", "decilitre"], "dl", Measure::Volume, 0.1), // i18n-ignore: unit names
    (&["l", "liter", "liters", "litre", "litres", "litri", "litro"], "l", Measure::Volume, 1.0), // i18n-ignore: unit names
    (&["m3", "m³", "cbm"], "m³", Measure::Volume, 1000.0), // i18n-ignore: unit names
    (&["tsp", "teaspoon", "teaspoons", "teelöffel", "tl"], "tsp", Measure::Volume, 0.00492892159375), // i18n-ignore: unit names
    (&["tbsp", "tablespoon", "tablespoons", "esslöffel", "el"], "tbsp", Measure::Volume, 0.01478676478125), // i18n-ignore: unit names
    (&["floz", "fl"], "fl oz", Measure::Volume, 0.0295735295625), // i18n-ignore: unit names
    (&["cup", "cups", "tasse", "tassen", "tazza", "tazze"], "cup", Measure::Volume, 0.2365882365), // i18n-ignore: unit names
    (&["pt", "pint", "pints"], "pt", Measure::Volume, 0.473176473), // i18n-ignore: unit names
    (&["qt", "quart", "quarts"], "qt", Measure::Volume, 0.946352946), // i18n-ignore: unit names
    (&["gal", "gallon", "gallons", "gallone", "gallonen", "galloni"], "gal", Measure::Volume, 3.785411784), // i18n-ignore: unit names
    (&["bit", "bits"], "bit", Measure::Data, 0.125), // i18n-ignore: unit names
    (&["b", "byte", "bytes"], "B", Measure::Data, 1.0), // i18n-ignore: unit names
    (&["kb", "kilobyte", "kilobytes"], "KB", Measure::Data, 1e3), // i18n-ignore: unit names
    (&["mb", "megabyte", "megabytes"], "MB", Measure::Data, 1e6), // i18n-ignore: unit names
    (&["gb", "gigabyte", "gigabytes"], "GB", Measure::Data, 1e9), // i18n-ignore: unit names
    (&["tb", "terabyte", "terabytes"], "TB", Measure::Data, 1e12), // i18n-ignore: unit names
    (&["pb", "petabyte", "petabytes"], "PB", Measure::Data, 1e15), // i18n-ignore: unit names
    (&["kib", "kibibyte"], "KiB", Measure::Data, 1024.0), // i18n-ignore: unit names
    (&["mib", "mebibyte"], "MiB", Measure::Data, 1048576.0), // i18n-ignore: unit names
    (&["gib", "gibibyte"], "GiB", Measure::Data, 1073741824.0), // i18n-ignore: unit names
    (&["tib", "tebibyte"], "TiB", Measure::Data, 1099511627776.0), // i18n-ignore: unit names
    (&["kbit", "kbits"], "kbit", Measure::Data, 125.0), // i18n-ignore: unit names
    (&["mbit", "mbits"], "Mbit", Measure::Data, 125000.0), // i18n-ignore: unit names
    (&["gbit", "gbits"], "Gbit", Measure::Data, 125000000.0), // i18n-ignore: unit names
    (&["ms", "millisecond", "milliseconds", "millisekunden", "millisecondi"], "ms", Measure::Time, 0.001), // i18n-ignore: unit names
    (&["s", "sec", "secs", "second", "seconds", "sekunde", "sekunden", "secondi", "secondo"], "s", Measure::Time, 1.0), // i18n-ignore: unit names
    (&["min", "mins", "minute", "minutes", "minuten", "minuti", "minuto"], "min", Measure::Time, 60.0), // i18n-ignore: unit names
    (&["h", "hr", "hrs", "hour", "hours", "stunde", "stunden", "std", "ora", "ore"], "h", Measure::Time, 3600.0), // i18n-ignore: unit names
    (&["d", "day", "days", "tag", "tage", "giorno", "giorni"], "d", Measure::Time, 86400.0), // i18n-ignore: unit names
    (&["wk", "week", "weeks", "woche", "wochen", "settimana", "settimane"], "wk", Measure::Time, 604800.0), // i18n-ignore: unit names
    (&["yr", "year", "years", "jahr", "jahre", "anno", "anni"], "yr", Measure::Time, 31556952.0), // i18n-ignore: unit names
];

fn unit(name: &str) -> Option<&'static (&'static [&'static str], &'static str, Measure, f64)> {
    UNITS.iter().find(|(names, ..)| names.contains(&name))
}

/// Celsius, Fahrenheit or Kelvin, by their place in the list, to Kelvin and back.
fn to_kelvin(value: f64, which: f64) -> f64 {
    match which as u8 {
        0 => value + 273.15,
        1 => (value - 32.0) * 5.0 / 9.0 + 273.15,
        _ => value,
    }
}

fn from_kelvin(value: f64, which: f64) -> f64 {
    match which as u8 {
        0 => value - 273.15,
        1 => (value - 273.15) * 9.0 / 5.0 + 32.0,
        _ => value,
    }
}

/// "5 km in mi", "70 f to c", "2 gib = mb", "3,5 kg in pfund", "1.5 h in min".
fn convert(text: &str) -> Option<Answer> {
    let lower = text.to_lowercase().replace("fl oz", "floz").replace("fl. oz", "floz"); // i18n-ignore: unit symbol
    let words: Vec<&str> = lower.split_whitespace().collect();
    let joiners = ["in", "to", "as", "into", "=", "nach", "zu", "a", "->"]; // i18n-ignore: words between units
    // The last joiner splits "what" from "into what": "5 in in cm" is inches in centimetres.
    let split = words.iter().rposition(|w| joiners.contains(w)).filter(|at| *at > 0 && *at + 1 < words.len())?;
    let to = words[split + 1..].join("");
    let from_text = words[..split].join(" ");
    // The amount, and the unit written after it with or without a space: "5km", "5 km".
    let unit_start = from_text.char_indices().rev().take_while(|(_, c)| c.is_alphabetic() || matches!(c, '°' | '"' | '³')).last().map(|(at, _)| at)?;
    let (amount, from) = (from_text[..unit_start].trim(), from_text[unit_start..].trim());
    let amount = if amount.is_empty() { 1.0 } else { number(amount).or_else(|| sum(amount).and_then(|a| a.copy.replace(',', ".").parse().ok()))? };
    let (from_unit, to_unit) = (unit(from)?, unit(&to)?);
    if from_unit.2 != to_unit.2 {
        return None;
    }
    let result = if from_unit.2 == Measure::Temperature {
        from_kelvin(to_kelvin(amount, from_unit.3), to_unit.3)
    } else {
        amount * from_unit.3 / to_unit.3
    };
    let space = |symbol: &str| if symbol.starts_with('°') { String::new() } else { " ".to_string() };
    Some(Answer {
        shown: format!("{}{}{} = {}{}{}", show_number(amount), space(from_unit.1), from_unit.1, show_number(result), space(to_unit.1), to_unit.1),
        copy: copy_number(result),
        kind: from_unit.2.label(),
    })
}

// ---- Dates ----

const WEEKDAYS: [&str; 7] = [tx("Monday"), tx("Tuesday"), tx("Wednesday"), tx("Thursday"), tx("Friday"), tx("Saturday"), tx("Sunday")];

/// Adds whole months, keeping the day where the month has it: 31 January and a month is 28 or 29 February.
fn add_months(day: Day, months: i64) -> Day {
    let index = day.year as i64 * 12 + day.month as i64 - 1 + months;
    let (year, month) = (index.div_euclid(12) as i32, index.rem_euclid(12) as u32 + 1);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let last = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    Day { year, month, day: day.day.min(last) }
}

/// "today + 30 days", "tomorrow", "heute - 2 wochen", "oggi + 1 mese 3 giorni", from the day `today`.
pub fn date_from(text: &str, today: Day) -> Option<Day> {
    let lower = text.to_lowercase().replace('+', " + ").replace('-', " - ");
    let mut words = lower.split_whitespace().peekable();
    let mut day = match words.next()? {
        "today" | "now" | "heute" | "jetzt" | "oggi" | "adesso" => today, // i18n-ignore: date words
        "tomorrow" | "morgen" | "domani" => today.offset(1), // i18n-ignore: date words
        "yesterday" | "gestern" | "ieri" => today.offset(-1), // i18n-ignore: date words
        _ => return None,
    };
    let mut sign = 1i64;
    while let Some(word) = words.next() {
        match word {
            "+" | "plus" | "und" | "and" | "e" | "più" => sign = 1, // i18n-ignore: date words
            "-" | "minus" | "meno" => sign = -1, // i18n-ignore: date words
            amount => {
                // Beyond a few hundred years there's no date to show, and nothing to overflow.
                let count = amount.parse::<i64>().ok().filter(|n| *n <= 100_000)?;
                let unit = words.next()?;
                let count = sign * count;
                day = match unit {
                    "d" | "day" | "days" | "tag" | "tage" | "tagen" | "giorno" | "giorni" => day.offset(count), // i18n-ignore: date words
                    "w" | "wk" | "week" | "weeks" | "woche" | "wochen" | "settimana" | "settimane" => day.offset(count * 7), // i18n-ignore: date words
                    "m" | "mo" | "month" | "months" | "monat" | "monate" | "monaten" | "mese" | "mesi" => add_months(day, count), // i18n-ignore: date words
                    "y" | "yr" | "year" | "years" | "jahr" | "jahre" | "jahren" | "anno" | "anni" => add_months(day, count * 12), // i18n-ignore: date words
                    _ => return None,
                };
            }
        }
    }
    (1..=9999).contains(&day.year).then_some(day)
}

fn date(text: &str) -> Option<Answer> {
    let today = Day::today();
    let day = date_from(text, today)?;
    // A word alone like "today" is a search too, unless it's worked out.
    if !text.contains(['+', '-']) && !text.chars().any(|c| c.is_ascii_digit()) && day == today {
        return None;
    }
    let shown = crate::i18n::fmt_date(day.year, day.month, day.day);
    let between = (day.number() - today.number()).abs();
    let ago = if day.number() >= today.number() { crate::trn!(between, "in {n} day", "in {n} days") } else { crate::trn!(between, "{n} day ago", "{n} days ago") };
    Some(Answer { shown: format!("{}, {shown} ({ago})", tr(WEEKDAYS[day.weekday() as usize])), copy: shown, kind: tx("Date") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Option<String> {
        answer(text).map(|a| a.copy)
    }

    #[test]
    fn sums_are_worked_out_and_plain_searches_left_alone() {
        assert_eq!(value("2 + 3 * 4"), Some("14".into()));
        assert_eq!(value("(2 + 3) * 4"), Some("20".into()));
        assert_eq!(value("2^3^2"), Some("512".into()));
        assert_eq!(value("-2^2"), Some("-4".into()));
        assert_eq!(value("10 / 4"), Some("2.5".into()));
        assert_eq!(value("1,5 * 2"), Some("3".into()), "a comma is a decimal mark too");
        assert_eq!(value("1,000.5 + 1"), Some("1001.5".into()));
        assert_eq!(value("200 * 15%"), Some("30".into()));
        assert_eq!(value("15% of 80"), Some("12".into()));
        assert_eq!(value("3 x 4"), Some("12".into()));
        assert_eq!(value("2(3 + 4)"), Some("14".into()));
        assert_eq!(value("sqrt 16 + 1"), Some("5".into()));
        assert_eq!(value("2pi"), Some("6.283185307".into()));
        assert_eq!(value("sin 30"), Some("0.5".into()), "angles in degrees");
        assert_eq!(value("0.1 + 0.2"), Some("0.3".into()));
        assert_eq!(answer("12 * 1000").unwrap().shown, "= 12,000");
        for search in ["42", "-5", "report 2024", "notepad", "c++", "3d print", "1/2 note", "x"] {
            assert_eq!(answer(search), None, "{search}");
        }
        assert_eq!(answer("1 / 0"), None, "nothing to show");
    }

    #[test]
    fn units_convert_within_what_they_measure() {
        let close = |text: &str, want: f64| {
            let got: f64 = value(text).unwrap_or_else(|| panic!("{text}")).parse().unwrap();
            assert!((got - want).abs() < want.abs() * 1e-6 + 1e-9, "{text}: {got} not {want}");
        };
        close("5 km in mi", 3.10685596);
        close("5km to m", 5000.0);
        close("12 in in cm", 30.48);
        close("6 ft to m", 1.8288);
        close("1 lb in g", 453.59237);
        close("100 f in c", 37.7777778);
        close("-40 c in f", -40.0);
        close("0 k to c", -273.15);
        close("2 gal in l", 7.570823568);
        close("500 ml to cups", 2.11337642);
        close("2 gib in mb", 2147.483648);
        close("100 mbit in mb", 12.5);
        close("90 min in h", 1.5);
        close("1 week in hours", 168.0);
        close("3,5 kg nach pfund", 7.71617917);
        assert_eq!(answer("5 km in kg"), None, "length isn't weight");
        assert_eq!(answer("cats in hats"), None);
        let shown = answer("100 f in c").unwrap();
        assert_eq!(shown.shown, "100°F = 37.77777778°C");
        assert_eq!(shown.kind, "Temperature");
    }

    #[test]
    fn dates_are_counted_from_today() {
        let today = Day { year: 2026, month: 1, day: 31 };
        assert_eq!(date_from("today + 30 days", today), Some(Day { year: 2026, month: 3, day: 2 }));
        assert_eq!(date_from("today+1 month", today), Some(Day { year: 2026, month: 2, day: 28 }), "the end of a shorter month");
        assert_eq!(date_from("today - 2 weeks", today), Some(Day { year: 2026, month: 1, day: 17 }));
        assert_eq!(date_from("tomorrow", today), Some(Day { year: 2026, month: 2, day: 1 }));
        assert_eq!(date_from("heute + 1 jahr", today), Some(Day { year: 2027, month: 1, day: 31 }));
        assert_eq!(date_from("oggi + 1 mese + 3 giorni", today), Some(Day { year: 2026, month: 3, day: 3 }));
        assert_eq!(date_from("today + many days", today), None);
        assert_eq!(date_from("todays news", today), None);
        assert_eq!(date_from("today + 99999999999999999 days", today), None, "nothing overflows");
        assert_eq!(date_from("today + 100000 years", today), None, "no year to show");
        assert_eq!(add_months(Day { year: 2024, month: 1, day: 31 }, 1), Day { year: 2024, month: 2, day: 29 }, "a leap year");
        assert!(answer("today").is_none(), "a word alone stays a search");
        assert!(answer("today + 3 days").is_some());
    }

    #[test]
    fn numbers_are_shown_the_way_each_language_writes_them() {
        assert_eq!(show_number_in(Lang::De, 1234567.5), "1.234.567,5");
        assert_eq!(show_number_in(Lang::It, 1234.5), "1234,5", "four digits stay together");
        assert_eq!(copy_number_in(Lang::De, 1234567.5), "1234567,5");
        assert_eq!(show_number_in(Lang::En, 1234567.5), "1,234,567.5");
        assert_eq!(show_number_in(Lang::En, 1.0 / 3.0), "0.3333333333");
        assert_eq!(show_number_in(Lang::En, -0.0), "0");
        assert_eq!(show_number_in(Lang::En, 1e20), "1e20");
    }
}
