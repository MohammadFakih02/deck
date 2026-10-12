//! Calculator and unit/currency conversion for the search box.

use parking_lot::Mutex;
use std::collections::HashMap;

// ------------------------------------------------------------ expressions

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Id(String),
    Op(char),
}

fn lex(s: &str) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            // 0x / 0b literals
            if ch == '0' && matches!(c.get(i + 1), Some('x') | Some('X') | Some('b') | Some('B')) {
                let radix = if matches!(c[i + 1], 'x' | 'X') { 16 } else { 2 };
                let mut j = i + 2;
                while j < c.len() && c[j].is_digit(radix) {
                    j += 1;
                }
                let t: String = c[i + 2..j].iter().collect();
                out.push(Tok::Num(i64::from_str_radix(&t, radix).ok()? as f64));
                i = j;
                continue;
            }
            let mut j = i;
            let mut t = String::new();
            while j < c.len() && (c[j].is_ascii_digit() || c[j] == '.' || c[j] == '_' || (c[j] == ',' && c.get(j + 1).is_some_and(|d| d.is_ascii_digit()) && c.get(j + 4).is_none_or(|d| !d.is_ascii_digit()) && c.get(j + 3).is_some_and(|d| d.is_ascii_digit()))) {
                if c[j] != '_' && c[j] != ',' {
                    t.push(c[j]);
                }
                j += 1;
            }
            // exponent
            if j < c.len() && (c[j] == 'e' || c[j] == 'E') && c.get(j + 1).is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '+') {
                let mut k = j + 1;
                let mut e = String::from("e");
                if c[k] == '-' || c[k] == '+' {
                    e.push(c[k]);
                    k += 1;
                }
                let start = k;
                while k < c.len() && c[k].is_ascii_digit() {
                    e.push(c[k]);
                    k += 1;
                }
                if k > start {
                    t.push_str(&e);
                    j = k;
                }
            }
            out.push(Tok::Num(t.parse().ok()?));
            i = j;
        } else if ch.is_alphabetic() || ch == 'π' {
            let mut j = i;
            let mut t = String::new();
            while j < c.len() && (c[j].is_alphanumeric() || c[j] == 'π') {
                t.push(c[j]);
                j += 1;
            }
            out.push(Tok::Id(t.to_lowercase()));
            i = j;
        } else if "+-*/^%()!,×÷−".contains(ch) {
            let op = match ch {
                '×' => '*',
                '÷' => '/',
                '−' => '-',
                o => o,
            };
            out.push(Tok::Op(op));
            i += 1;
        } else {
            return None;
        }
    }
    Some(out)
}

struct P {
    t: Vec<Tok>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn eat(&mut self, op: char) -> bool {
        if self.peek() == Some(&Tok::Op(op)) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            // "200 + 10%" adds ten percent of 200 (like a desk calculator), "200 - 10%" takes it off
            if self.eat('+') {
                let (t, pct) = self.term_or_pct()?;
                v += if pct { v * t } else { t };
            } else if self.eat('-') {
                let (t, pct) = self.term_or_pct()?;
                v -= if pct { v * t } else { t };
            } else {
                return Some(v);
            }
        }
    }
    /// A term, and whether it was a bare percentage ("10%", already divided by 100).
    fn term_or_pct(&mut self) -> Option<(f64, bool)> {
        let at = self.i;
        let v = self.term()?;
        let pct = self.i == at + 2 && matches!(self.t.get(at), Some(Tok::Num(_))) && self.t.get(at + 1) == Some(&Tok::Op('%'));
        Some((v, pct))
    }
    fn starts_value(&self) -> bool {
        matches!(self.peek(), Some(Tok::Num(_)) | Some(Tok::Id(_)) | Some(Tok::Op('(')))
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        loop {
            if self.eat('*') {
                v *= self.unary()?;
            } else if self.eat('/') {
                v /= self.unary()?;
            } else if self.peek() == Some(&Tok::Id("x".into())) {
                self.i += 1;
                v *= self.unary()?;
            } else if self.peek() == Some(&Tok::Id("mod".into())) {
                self.i += 1;
                v %= self.unary()?;
            } else if self.peek() == Some(&Tok::Op('%')) {
                // "a % b" is modulo; a trailing "a%" is a percentage.
                self.i += 1;
                if self.peek() == Some(&Tok::Id("of".into())) {
                    self.i += 1;
                    v = v / 100.0 * self.unary()?;
                } else if self.starts_value() {
                    v %= self.unary()?;
                } else {
                    v /= 100.0;
                }
            } else if matches!(self.peek(), Some(Tok::Op('('))) || matches!(self.peek(), Some(Tok::Id(id)) if id != "of" && id != "to" && id != "in") {
                // implicit multiplication: 2(3+4), 2pi
                v *= self.unary()?;
            } else {
                return Some(v);
            }
        }
    }
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
        let b = self.postfix()?;
        if self.eat('^') {
            let e = self.unary()?;
            return Some(b.powf(e));
        }
        Some(b)
    }
    fn postfix(&mut self) -> Option<f64> {
        let mut v = self.primary()?;
        while self.eat('!') {
            if v < 0.0 || v > 170.0 || v.fract() != 0.0 {
                return None;
            }
            v = (1..=v as u64).fold(1.0, |a, b| a * b as f64);
        }
        Some(v)
    }
    fn args(&mut self) -> Option<Vec<f64>> {
        if !self.eat('(') {
            // function without parentheses: sqrt 16
            return Some(vec![self.power()?]);
        }
        let mut a = vec![self.expr()?];
        while self.eat(',') {
            a.push(self.expr()?);
        }
        self.eat(')').then_some(a)
    }
    fn primary(&mut self) -> Option<f64> {
        match self.peek()?.clone() {
            Tok::Num(n) => {
                self.i += 1;
                Some(n)
            }
            Tok::Op('(') => {
                self.i += 1;
                let v = self.expr()?;
                self.eat(')');
                Some(v)
            }
            Tok::Id(id) => {
                self.i += 1;
                let one = |p: &mut P, f: fn(f64) -> f64| -> Option<f64> {
                    let a = p.args()?;
                    (a.len() == 1).then(|| f(a[0]))
                };
                match id.as_str() {
                    "pi" | "π" => Some(std::f64::consts::PI),
                    "e" => Some(std::f64::consts::E),
                    "tau" => Some(std::f64::consts::TAU),
                    "sqrt" => one(self, f64::sqrt),
                    "cbrt" => one(self, f64::cbrt),
                    "abs" => one(self, f64::abs),
                    "round" => one(self, f64::round),
                    "floor" => one(self, f64::floor),
                    "ceil" => one(self, f64::ceil),
                    "sin" => one(self, |x| x.to_radians().sin()),
                    "cos" => one(self, |x| x.to_radians().cos()),
                    "tan" => one(self, |x| x.to_radians().tan()),
                    "asin" => one(self, |x| x.asin().to_degrees()),
                    "acos" => one(self, |x| x.acos().to_degrees()),
                    "atan" => one(self, |x| x.atan().to_degrees()),
                    "ln" => one(self, f64::ln),
                    "log" => one(self, f64::log10),
                    "log2" => one(self, f64::log2),
                    "exp" => one(self, f64::exp),
                    "min" => self.args().filter(|a| !a.is_empty()).map(|a| a.into_iter().fold(f64::INFINITY, f64::min)),
                    "max" => self.args().filter(|a| !a.is_empty()).map(|a| a.into_iter().fold(f64::NEG_INFINITY, f64::max)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// Evaluates an arithmetic expression. Returns None for anything that is
/// not clearly a calculation (plain numbers, words).
pub fn eval(s: &str) -> Option<f64> {
    let s = s.trim().trim_start_matches('=').trim();
    if s.is_empty() {
        return None;
    }
    let t = lex(s)?;
    let has_op = t.iter().any(|x| matches!(x, Tok::Op(o) if *o != '(' && *o != ')'))
        || t.iter().any(|x| matches!(x, Tok::Id(_)));
    let has_num = t.iter().any(|x| matches!(x, Tok::Num(_)) || matches!(x, Tok::Id(i) if i == "pi" || i == "π" || i == "e" || i == "tau"));
    if !has_op || !has_num {
        return None;
    }
    let mut p = P { t, i: 0 };
    let v = p.expr()?;
    (p.i == p.t.len() && v.is_finite()).then_some(v)
}

/// Formats a result: up to 10 significant decimals, thousands separators.
pub fn fmt_num(v: f64) -> String {
    if v != 0.0 && (v.abs() >= 1e15 || v.abs() < 1e-9) {
        return format!("{v:e}");
    }
    let s = format!("{:.10}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    let (int, frac) = s.split_once('.').map(|(a, b)| (a, Some(b))).unwrap_or((s, None));
    let neg = int.starts_with('-');
    let digits = int.trim_start_matches('-');
    let mut grouped = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let mut out = if neg { format!("-{grouped}") } else { grouped };
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

// ------------------------------------------------------------ conversions

struct Unit {
    names: &'static [&'static str],
    cat: &'static str,
    /// value in base unit = x * factor (temperatures handled separately)
    factor: f64,
    label: &'static str,
}

const UNITS: &[Unit] = &[
    // length (m)
    Unit { names: &["mm", "millimeter", "millimeters", "millimetre", "millimetres"], cat: "length", factor: 0.001, label: "mm" },
    Unit { names: &["cm", "centimeter", "centimeters", "centimetre", "centimetres"], cat: "length", factor: 0.01, label: "cm" },
    Unit { names: &["m", "meter", "meters", "metre", "metres"], cat: "length", factor: 1.0, label: "m" },
    Unit { names: &["km", "kilometer", "kilometers", "kilometre", "kilometres"], cat: "length", factor: 1000.0, label: "km" },
    Unit { names: &["in", "inch", "inches", "\""], cat: "length", factor: 0.0254, label: "in" },
    Unit { names: &["ft", "foot", "feet", "'"], cat: "length", factor: 0.3048, label: "ft" },
    Unit { names: &["yd", "yard", "yards"], cat: "length", factor: 0.9144, label: "yd" },
    Unit { names: &["mi", "mile", "miles"], cat: "length", factor: 1609.344, label: "mi" },
    Unit { names: &["nmi", "nauticalmile", "nauticalmiles"], cat: "length", factor: 1852.0, label: "nmi" },
    // mass (kg)
    Unit { names: &["mg", "milligram", "milligrams"], cat: "mass", factor: 1e-6, label: "mg" },
    Unit { names: &["g", "gram", "grams"], cat: "mass", factor: 0.001, label: "g" },
    Unit { names: &["kg", "kilo", "kilos", "kilogram", "kilograms"], cat: "mass", factor: 1.0, label: "kg" },
    Unit { names: &["t", "ton", "tons", "tonne", "tonnes"], cat: "mass", factor: 1000.0, label: "t" },
    Unit { names: &["oz", "ounce", "ounces"], cat: "mass", factor: 0.028349523125, label: "oz" },
    Unit { names: &["lb", "lbs", "pound", "pounds"], cat: "mass", factor: 0.45359237, label: "lb" },
    Unit { names: &["st", "stone", "stones"], cat: "mass", factor: 6.35029318, label: "st" },
    // volume (l)
    Unit { names: &["ml", "milliliter", "milliliters", "millilitre", "millilitres"], cat: "volume", factor: 0.001, label: "ml" },
    Unit { names: &["cl"], cat: "volume", factor: 0.01, label: "cl" },
    Unit { names: &["l", "liter", "liters", "litre", "litres"], cat: "volume", factor: 1.0, label: "l" },
    Unit { names: &["gal", "gallon", "gallons"], cat: "volume", factor: 3.785411784, label: "gal" },
    Unit { names: &["qt", "quart", "quarts"], cat: "volume", factor: 0.946352946, label: "qt" },
    Unit { names: &["pt", "pint", "pints"], cat: "volume", factor: 0.473176473, label: "pt" },
    Unit { names: &["cup", "cups"], cat: "volume", factor: 0.2365882365, label: "cups" },
    Unit { names: &["floz", "fl oz"], cat: "volume", factor: 0.0295735295625, label: "fl oz" },
    Unit { names: &["tbsp", "tablespoon", "tablespoons"], cat: "volume", factor: 0.01478676478125, label: "tbsp" },
    Unit { names: &["tsp", "teaspoon", "teaspoons"], cat: "volume", factor: 0.00492892159375, label: "tsp" },
    // area (m2)
    Unit { names: &["m2", "sqm"], cat: "area", factor: 1.0, label: "m²" },
    Unit { names: &["km2", "sqkm"], cat: "area", factor: 1e6, label: "km²" },
    Unit { names: &["ft2", "sqft"], cat: "area", factor: 0.09290304, label: "ft²" },
    Unit { names: &["acre", "acres"], cat: "area", factor: 4046.8564224, label: "acres" },
    Unit { names: &["ha", "hectare", "hectares"], cat: "area", factor: 10000.0, label: "ha" },
    // time (s)
    Unit { names: &["ms", "millisecond", "milliseconds"], cat: "time", factor: 0.001, label: "ms" },
    Unit { names: &["s", "sec", "secs", "second", "seconds"], cat: "time", factor: 1.0, label: "s" },
    Unit { names: &["min", "mins", "minute", "minutes"], cat: "time", factor: 60.0, label: "min" },
    Unit { names: &["h", "hr", "hrs", "hour", "hours"], cat: "time", factor: 3600.0, label: "h" },
    Unit { names: &["d", "day", "days"], cat: "time", factor: 86400.0, label: "days" },
    Unit { names: &["wk", "week", "weeks"], cat: "time", factor: 604800.0, label: "weeks" },
    Unit { names: &["yr", "year", "years"], cat: "time", factor: 31557600.0, label: "years" },
    // data (bytes)
    Unit { names: &["b", "byte", "bytes"], cat: "data", factor: 1.0, label: "B" },
    Unit { names: &["kb", "kilobyte", "kilobytes"], cat: "data", factor: 1e3, label: "KB" },
    Unit { names: &["mb", "megabyte", "megabytes"], cat: "data", factor: 1e6, label: "MB" },
    Unit { names: &["gb", "gigabyte", "gigabytes"], cat: "data", factor: 1e9, label: "GB" },
    Unit { names: &["tb", "terabyte", "terabytes"], cat: "data", factor: 1e12, label: "TB" },
    Unit { names: &["kib"], cat: "data", factor: 1024.0, label: "KiB" },
    Unit { names: &["mib"], cat: "data", factor: 1048576.0, label: "MiB" },
    Unit { names: &["gib"], cat: "data", factor: 1073741824.0, label: "GiB" },
    Unit { names: &["tib"], cat: "data", factor: 1099511627776.0, label: "TiB" },
    Unit { names: &["bit", "bits"], cat: "data", factor: 0.125, label: "bits" },
    Unit { names: &["mbit", "megabit", "megabits"], cat: "data", factor: 125000.0, label: "Mbit" },
    Unit { names: &["gbit", "gigabit", "gigabits"], cat: "data", factor: 125000000.0, label: "Gbit" },
    // speed (m/s)
    Unit { names: &["m/s", "mps"], cat: "speed", factor: 1.0, label: "m/s" },
    Unit { names: &["km/h", "kmh", "kph"], cat: "speed", factor: 1.0 / 3.6, label: "km/h" },
    Unit { names: &["mph"], cat: "speed", factor: 0.44704, label: "mph" },
    Unit { names: &["kn", "knot", "knots"], cat: "speed", factor: 0.514444, label: "knots" },
    // data rate (bytes/s)
    Unit { names: &["mbps", "mbit/s"], cat: "rate", factor: 125000.0, label: "Mbps" },
    Unit { names: &["gbps", "gbit/s"], cat: "rate", factor: 125000000.0, label: "Gbps" },
    Unit { names: &["mb/s", "mbs"], cat: "rate", factor: 1e6, label: "MB/s" },
    // energy (J)
    Unit { names: &["j", "joule", "joules"], cat: "energy", factor: 1.0, label: "J" },
    Unit { names: &["kj"], cat: "energy", factor: 1000.0, label: "kJ" },
    Unit { names: &["cal", "calorie", "calories"], cat: "energy", factor: 4.184, label: "cal" },
    Unit { names: &["kcal"], cat: "energy", factor: 4184.0, label: "kcal" },
    Unit { names: &["kwh"], cat: "energy", factor: 3.6e6, label: "kWh" },
];

fn unit(name: &str) -> Option<&'static Unit> {
    let n = name.to_lowercase();
    UNITS.iter().find(|u| u.names.contains(&n.as_str()))
}

fn temp(name: &str) -> Option<char> {
    match name.to_lowercase().trim_start_matches('°') {
        "c" | "celsius" | "degc" => Some('C'),
        "f" | "fahrenheit" | "degf" => Some('F'),
        "k" | "kelvin" => Some('K'),
        _ => None,
    }
}

// ------------------------------------------------------------ currency

pub struct Rates {
    /// (fetched unix s, USD -> rate)
    cache: Mutex<(u64, HashMap<String, f64>)>,
}

const CURRENCY_ALIASES: &[(&str, &str)] = &[
    ("$", "USD"), ("dollar", "USD"), ("dollars", "USD"), ("usd", "USD"),
    ("€", "EUR"), ("euro", "EUR"), ("euros", "EUR"),
    ("£", "GBP"), ("pound sterling", "GBP"), ("quid", "GBP"),
    ("¥", "JPY"), ("yen", "JPY"),
    ("dirham", "AED"), ("dirhams", "AED"), ("riyal", "SAR"), ("riyals", "SAR"), ("dinar", "KWD"), ("dinars", "KWD"),
    ("lira", "TRY"), ("rupee", "INR"), ("rupees", "INR"), ("yuan", "CNY"), ("won", "KRW"), ("franc", "CHF"), ("francs", "CHF"),
];

impl Rates {
    pub fn new() -> Self {
        let file = crate::settings::Settings::dir().join("rates.json");
        let cache = std::fs::read_to_string(file)
            .ok()
            .and_then(|t| serde_json::from_str::<(u64, HashMap<String, f64>)>(&t).ok())
            .unwrap_or_default();
        Self { cache: Mutex::new(cache) }
    }

    fn rates(&self) -> Option<HashMap<String, f64>> {
        {
            let c = self.cache.lock();
            if !c.1.is_empty() && crate::util::now_s().saturating_sub(c.0) < 12 * 3600 {
                return Some(c.1.clone());
            }
        }
        let fresh = ureq::get("https://open.er-api.com/v6/latest/USD")
            .timeout(std::time::Duration::from_secs(4))
            .call()
            .ok()
            .and_then(|r| r.into_json::<serde_json::Value>().ok())
            .and_then(|v| {
                v.get("rates")?.as_object().map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect::<HashMap<_, _>>())
            });
        let mut c = self.cache.lock();
        if let Some(r) = fresh.filter(|r| r.len() > 20) {
            *c = (crate::util::now_s(), r);
            let _ = std::fs::create_dir_all(crate::settings::Settings::dir());
            let _ = std::fs::write(crate::settings::Settings::dir().join("rates.json"), serde_json::to_string(&*c).unwrap_or_default());
        }
        (!c.1.is_empty()).then(|| c.1.clone())
    }

    fn code(&self, s: &str) -> Option<String> {
        let l = s.to_lowercase();
        if let Some((_, c)) = CURRENCY_ALIASES.iter().find(|(a, _)| *a == l) {
            return Some(c.to_string());
        }
        (s.len() == 3 && s.chars().all(|c| c.is_ascii_alphabetic())).then(|| s.to_uppercase())
    }
}

/// A conversion result: (answer text, detail line).
pub fn convert(q: &str, rates: &Rates) -> Option<(String, String)> {
    let q = q.trim();
    let lower = q.to_lowercase();
    let (lhs, rhs) = [" to ", " in ", " as ", " = ", " -> "]
        .iter()
        .find_map(|sep| lower.rfind(sep).map(|i| (lower[..i].trim().to_string(), lower[i + sep.len()..].trim().to_string())))?;
    // split lhs into number + unit ("12km", "12 km", "$20", "20$")
    let lhs_l = lhs.replace(' ', "");
    let (num, from) = {
        let start_num = lhs_l.find(|c: char| c.is_ascii_digit() || c == '.' || c == '-')?;
        let end_num = lhs_l[start_num..].find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == ',')).map(|e| e + start_num).unwrap_or(lhs_l.len());
        let n: f64 = lhs_l[start_num..end_num].replace(',', "").parse().ok()?;
        let unit_s = format!("{}{}", &lhs_l[..start_num], &lhs_l[end_num..]);
        (n, unit_s)
    };
    let to = rhs.replace(' ', "");
    if from.is_empty() || to.is_empty() {
        return None;
    }
    if let (Some(a), Some(b)) = (temp(&from), temp(&to)) {
        let c = match a {
            'C' => num,
            'F' => (num - 32.0) * 5.0 / 9.0,
            _ => num - 273.15,
        };
        let r = match b {
            'C' => c,
            'F' => c * 9.0 / 5.0 + 32.0,
            _ => c + 273.15,
        };
        let unit_label = |u: char| if u == 'K' { " K".to_string() } else { format!(" °{u}") };
        return Some((format!("{}{}", fmt_num((r * 100.0).round() / 100.0), unit_label(b)), format!("{}{} in °{}", fmt_num(num), unit_label(a), b)));
    }
    if let (Some(a), Some(b)) = (unit(&from), unit(&to)) {
        if a.cat != b.cat {
            return None;
        }
        let r = num * a.factor / b.factor;
        let r = if r.abs() >= 1.0 { (r * 1e6).round() / 1e6 } else { r };
        return Some((format!("{} {}", fmt_num(r), b.label), format!("{} {} = {} {}", fmt_num(num), a.label, fmt_num(r), b.label)));
    }
    let (a, b) = (rates.code(&from)?, rates.code(&to)?);
    let r = rates.rates()?;
    let (ra, rb) = (r.get(&a)?, r.get(&b)?);
    let v = num / ra * rb;
    Some((format!("{} {}", fmt_num((v * 100.0).round() / 100.0), b), format!("{} {} in {} · daily rate", fmt_num(num), a, b)))
}

/// "255 to hex", "0xff to dec", "10 to bin".
pub fn base_convert(q: &str) -> Option<(String, String)> {
    let l = q.trim().to_lowercase();
    let (n, to) = l.rsplit_once(" to ").or_else(|| l.rsplit_once(" in "))?;
    let v = eval(&format!("{n}+0"))?;
    if v.fract() != 0.0 || v.abs() > 9.0e15 {
        return None;
    }
    let v = v as i64;
    let out = match to.trim() {
        "hex" => format!("0x{v:X}"),
        "bin" | "binary" => format!("0b{v:b}"),
        "oct" | "octal" => format!("0o{v:o}"),
        "dec" | "decimal" => v.to_string(),
        _ => return None,
    };
    Some((out, format!("{} in {}", n.trim(), to.trim())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math() {
        assert_eq!(eval("2+3*4"), Some(14.0));
        assert_eq!(eval("(2+3)*4"), Some(20.0));
        assert_eq!(eval("2^10"), Some(1024.0));
        assert_eq!(eval("-2^2"), Some(-4.0));
        assert_eq!(eval("10 % 3"), Some(1.0));
        assert_eq!(eval("20% of 50"), Some(10.0));
        assert_eq!(eval("50*10%"), Some(5.0));
        assert_eq!(eval("200 + 10%"), Some(220.0));
        assert_eq!(eval("200 - 25%"), Some(150.0));
        assert_eq!(eval("200 + 10% of 50"), Some(205.0));
        assert_eq!(eval("7 + 10 % 3"), Some(8.0));
        assert_eq!(eval("sqrt(16)+1"), Some(5.0));
        assert_eq!(eval("5!"), Some(120.0));
        assert_eq!(eval("2pi").map(|v| (v * 1000.0).round()), Some(6283.0));
        assert_eq!(eval("1,000*2"), Some(2000.0));
        assert_eq!(eval("0xff+1"), Some(256.0));
        assert_eq!(eval("12 x 3"), Some(36.0));
        assert_eq!(eval("max(3, 9, 2)"), Some(9.0));
        assert_eq!(eval("5"), None);
        assert_eq!(eval("hello"), None);
        assert_eq!(eval("discord"), None);
        assert_eq!(fmt_num(1234567.5), "1,234,567.5");
        assert_eq!(fmt_num(-0.25), "-0.25");
    }

    #[test]
    fn units() {
        let r = Rates { cache: Mutex::new((u64::MAX / 2, HashMap::from([("USD".to_string(), 1.0), ("EUR".to_string(), 0.5)]))) };
        assert_eq!(convert("5 km to miles", &r).unwrap().0, "3.106856 mi");
        assert_eq!(convert("100 f to c", &r).unwrap().0, "37.78 °C");
        assert_eq!(convert("1 gb in mb", &r).unwrap().0, "1,000 MB");
        assert_eq!(convert("20 usd to eur", &r).unwrap().0, "10 EUR");
        assert_eq!(convert("$20 in euro", &r).unwrap().0, "10 EUR");
        assert!(convert("5 km to kg", &r).is_none());
        assert_eq!(base_convert("255 to hex").unwrap().0, "0xFF");
        assert_eq!(base_convert("0xff to dec").unwrap().0, "255");
    }
}

