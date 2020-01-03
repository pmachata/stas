extern crate fixed;
extern crate glob;

use std::iter::Peekable;

// Counters are generally 64-bit quantities. To support displaying deltas up to that resolution, we
// need an extra bit. And then to represent fractional values based off a 64-bit quantity, we need
// more bits for the fraction. To keep things simple, use a 128-bit fixpoint value split to 65 bits
// interal part and 63 bits fractional.
pub type Value = fixed::types::I65F63;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum UBase {
    Units,
    Packets,
    Seconds,
    Bits,
    Bytes,
}

static UNITS: [(UBase, char); 5] = [
    (UBase::Units, '1'),
    (UBase::Packets, 'p'),
    (UBase::Seconds, 's'),
    (UBase::Bits, 'b'),
    (UBase::Bytes, 'B'),
];

impl std::string::ToString for UBase {
    fn to_string(&self) -> String {
        UNITS
            .iter()
            .find(|&(unit, _)| unit == self)
            .unwrap()
            .1
            .to_string()
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum UPfx {
    Nano,
    Micro,
    Milli,
    None,
    Kilo,
    Mega,
    Giga,
    Tera,
    Peta,
    Exa,
}

static PREFIXES: [(UPfx, char); 10] = [
    (UPfx::Nano, 'n'),
    (UPfx::Micro, 'u'),
    (UPfx::Milli, 'm'),
    (UPfx::None, ' '),
    (UPfx::Kilo, 'K'),
    (UPfx::Mega, 'M'),
    (UPfx::Giga, 'G'),
    (UPfx::Tera, 'T'),
    (UPfx::Peta, 'P'),
    (UPfx::Exa, 'E'),
];

#[derive(Clone, Debug)]
pub struct Unit {
    pub prefix: UPfx,
    pub base: UBase,
}

#[derive(PartialEq, Clone, Debug)]
pub enum UFreq {
    AsIs,
    Delta,
    PerSecond,
}

#[derive(Clone, Debug)]
pub struct UnitChain {
    pub units: Vec<Unit>,
    pub freq: UFreq,
}

pub fn humanize(mut value: Value, base: UPfx, unit_prefix_str: &str, unit_str: &String) -> String {
    let mut pos = PREFIXES.iter().position(|(unit, _)| *unit == base).unwrap();
    let mut trivial = true;

    while value.abs() >= 1100 && (pos + 1) < PREFIXES.len() {
        value /= 1000;
        pos += 1;
        trivial = false;
    }

    if trivial {
        format!(
            "{}{:.0}    {}{}",
            unit_prefix_str, value, PREFIXES[pos].1, &unit_str
        )
    } else {
        format!(
            "{}{:.2} {}{}",
            unit_prefix_str, value, PREFIXES[pos].1, &unit_str
        )
    }
}

fn parse_unit_pfx<I>(it: &mut Peekable<I>) -> Result<Unit, String>
where
    I: Iterator<Item = char>,
{
    let prefix = match it.peek() {
        Some(&'G') => {
            it.next();
            UPfx::Giga
        }
        Some(&'M') => {
            it.next();
            UPfx::Mega
        }
        Some(&'k') | Some(&'K') => {
            it.next();
            UPfx::Kilo
        }
        Some(&'m') => {
            it.next();
            UPfx::Milli
        }
        Some(&'u') => {
            it.next();
            UPfx::Micro
        }
        Some(&'n') => {
            it.next();
            UPfx::Nano
        }
        _ => UPfx::None,
    };

    let base = match it.next() {
        Some('p') => UBase::Packets,
        Some('s') => UBase::Seconds,
        Some('B') => UBase::Bytes,
        Some('b') => UBase::Bits,
        Some('1') => UBase::Units,
        Some(c) => {
            return Err(format!("Unknown unit, '{}'", c));
        }
        _ => {
            return Err("Missing unit".to_string());
        }
    };

    Ok(Unit {
        prefix: prefix,
        base: base,
    })
}

fn parse_unit_freq(str: &str) -> Result<(Unit, UFreq), String> {
    let mut freq: Option<UFreq> = None;
    let mut it = str.chars().peekable();

    if it.peek() == Some(&'d') {
        it.next();
        freq = Some(UFreq::Delta);
    }

    let pfx = parse_unit_pfx(&mut it)?;

    let rest = it.collect::<String>();
    if rest.is_empty() {
        return Ok((pfx, freq.unwrap_or(UFreq::AsIs)));
    }

    if freq.is_some() {
        return Err(format!("Unit suffix not expected: {}", rest));
    }

    if rest == "ps" {
        return Ok((pfx, UFreq::PerSecond));
    }

    return Err(format!("Unit suffix not understood: {}", rest));
}

fn parse_unit_chain(str: &str) -> Result<UnitChain, String> {
    let mut units = Vec::<Unit>::new();
    let mut freq = UFreq::AsIs;

    // The unit string starts with a '/', so skip the first (empty) element.
    for substr in str.split('/').skip(1) {
        let (unit, this_freq) = parse_unit_freq(substr)?;
        if this_freq != UFreq::AsIs {
            if freq != UFreq::AsIs {
                return Err("Only one frequency allowed in a unit chain.".to_string());
            }
            freq = this_freq;
        }
        units.push(unit);
    }

    Ok(UnitChain {
        units: units,
        freq: freq,
    })
}

pub fn parse_unit(str: &String) -> Result<Option<UnitChain>, String> {
    if str.is_empty() || !str.starts_with('/') {
        return Ok(None);
    }
    Ok(Some(parse_unit_chain(str)?))
}

// xxx temporary -- will be replaced by a trait
pub struct CounterRule {
    pub pat: glob::Pattern,
    pub unit: Option<UnitChain>,
}

pub fn convert(
    uchain: &UnitChain,
    value: Value,
    avg: Option<Value>,
) -> (Value, Option<Value>, Unit) {
    assert!(!uchain.units.is_empty());

    let mut it = uchain.units.iter();
    let mut prev_unit = it.next().unwrap();
    let ret_prefix = prev_unit.prefix;
    let mut ret_value = value;
    let mut ret_avg = avg;

    for unit in it {
        match (prev_unit.base, unit.base) {
            (UBase::Bytes, UBase::Bits) => {
                ret_value *= 8;
                ret_avg = ret_avg.map(|avalue| avalue * 8);
            }
            (UBase::Bits, UBase::Bytes) => {
                ret_value /= 8;
                ret_avg = avg.map(|avalue| avalue / 8);
            }
            _ => {}
        }
        prev_unit = unit;
    }

    (
        ret_value,
        ret_avg,
        Unit {
            prefix: ret_prefix,
            base: prev_unit.base,
        },
    )
}
