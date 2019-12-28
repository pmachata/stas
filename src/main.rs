extern crate fixed;
extern crate glob;
extern crate termion;

use std::env;
use std::io::{stdout, Write};
use std::iter::Peekable;
use std::thread;
use std::time;

mod ethtool_ss;
mod netlink;

// Counters are generally 64-bit quantities. To support displaying deltas up to that resolution, we
// need an extra bit. And then to represent fractional values based off a 64-bit quantity, we need
// more bits for the fraction. To keep things simple, use a 128-bit fixpoint value split to 65 bits
// interal part and 63 bits fractional.
type CtVal = fixed::types::I65F63;

#[derive(Eq, PartialEq)]
struct CounterKey {
    ifname: String,
    ctns: String,
    ctname: String,
}

#[derive(Clone, Copy, PartialEq)]
enum UnitBase {
    Units,
    Packets,
    Seconds,
    Bits,
    Bytes,
}

static UNITS: [(UnitBase, char); 5] = [
    (UnitBase::Units, '1'),
    (UnitBase::Packets, 'p'),
    (UnitBase::Seconds, 's'),
    (UnitBase::Bits, 'b'),
    (UnitBase::Bytes, 'B'),
];

#[derive(Clone, Copy, Eq, PartialEq)]
enum UnitPrefix {
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

static PREFIXES: [(UnitPrefix, char); 10] = [
    (UnitPrefix::Nano, 'n'),
    (UnitPrefix::Micro, 'u'),
    (UnitPrefix::Milli, 'm'),
    (UnitPrefix::None, ' '),
    (UnitPrefix::Kilo, 'K'),
    (UnitPrefix::Mega, 'M'),
    (UnitPrefix::Giga, 'G'),
    (UnitPrefix::Tera, 'T'),
    (UnitPrefix::Peta, 'P'),
    (UnitPrefix::Exa, 'E'),
];

#[derive(Clone)]
struct Unit {
    prefix: UnitPrefix,
    unit: UnitBase,
}

#[derive(PartialEq, Clone)]
enum UnitFrequency {
    AsIs,
    Delta,
    PerSecond,
}

#[derive(Clone)]
struct UnitChain {
    units: Vec<Unit>,
    freq: UnitFrequency,
}

fn humanize(
    mut value: CtVal,
    base: UnitPrefix,
    unit_prefix_str: &str,
    unit_str: &String,
) -> String {
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
            UnitPrefix::Giga
        }
        Some(&'M') => {
            it.next();
            UnitPrefix::Mega
        }
        Some(&'k') | Some(&'K') => {
            it.next();
            UnitPrefix::Kilo
        }
        Some(&'m') => {
            it.next();
            UnitPrefix::Milli
        }
        Some(&'u') => {
            it.next();
            UnitPrefix::Micro
        }
        Some(&'n') => {
            it.next();
            UnitPrefix::Nano
        }
        _ => UnitPrefix::None,
    };

    let unit = match it.next() {
        Some('p') => UnitBase::Packets,
        Some('s') => UnitBase::Seconds,
        Some('B') => UnitBase::Bytes,
        Some('b') => UnitBase::Bits,
        Some('1') => UnitBase::Units,
        Some(c) => {
            return Err(format!("Unknown unit, '{}'", c));
        }
        _ => {
            return Err("Missing unit".to_string());
        }
    };

    Ok(Unit {
        prefix: prefix,
        unit: unit,
    })
}

fn parse_unit_freq(str: &str) -> Result<(Unit, UnitFrequency), String> {
    let mut freq: Option<UnitFrequency> = None;
    let mut it = str.chars().peekable();

    if it.peek() == Some(&'d') {
        it.next();
        freq = Some(UnitFrequency::Delta);
    }

    let pfx = parse_unit_pfx(&mut it)?;

    let rest = it.collect::<String>();
    if rest.is_empty() {
        return Ok((pfx, freq.unwrap_or(UnitFrequency::AsIs)));
    }

    if freq.is_some() {
        return Err(format!("Unit suffix not expected: {}", rest));
    }

    if rest == "ps" {
        return Ok((pfx, UnitFrequency::PerSecond));
    }

    return Err(format!("Unit suffix not understood: {}", rest));
}

fn parse_unit_chain(str: &str) -> Result<UnitChain, String> {
    let mut units = Vec::<Unit>::new();
    let mut freq = UnitFrequency::AsIs;

    // The unit string starts with a '/', so skip the first (empty) element.
    for substr in str.split('/').skip(1) {
        let (unit, this_freq) = parse_unit_freq(substr)?;
        if this_freq != UnitFrequency::AsIs {
            if freq != UnitFrequency::AsIs {
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

fn parse_unit(str: &String) -> Result<Option<UnitChain>, String> {
    if str.is_empty() || !str.starts_with('/') {
        return Ok(None);
    }
    Ok(Some(parse_unit_chain(str)?))
}

struct CounterRule {
    pat: glob::Pattern,
    unit: Option<UnitChain>,
}

struct CounterHistory {
    key: CounterKey,
    history: Vec<u64>,
    base: u64,
    age: u32,
    unit: UnitChain,
}

fn main() {
    let ifmatch: glob::Pattern;
    let rules: Vec<CounterRule>;

    {
        let mut args: Vec<String> = env::args().collect();
        if args.len() <= 2 {
            println!("Usage: {} <if> <counter> [<counter> ...]", args[0]);
            std::process::exit(1);
        }
        args.remove(0);

        // Add an implicit unit for any counters left without one.
        args.push("/1".to_string());

        let arg0 = args.remove(0);
        if let Ok(pat) = glob::Pattern::new(&arg0) {
            ifmatch = pat;
        } else {
            println!("Interface match expected, e.g. 'eth0' or 'eth*'.");
            std::process::exit(1);
        }

        let mut _rules = Vec::<CounterRule>::new();
        for arg in args {
            match parse_unit(&arg) {
                Ok(Some(_unit_chain)) => {
                    for rule in _rules.iter_mut().rev() {
                        if rule.unit.is_none() {
                            rule.unit = Some(_unit_chain.clone());
                        } else {
                            break;
                        }
                    }
                    continue;
                }
                Err(e) => {
                    println!("Error parsing {}: {}", arg, e);
                    return;
                }
                Ok(None) => {}
            }

            if let Ok(pat) = glob::Pattern::new(&arg) {
                _rules.push(CounterRule {
                    pat: pat,
                    unit: None,
                });
            } else {
                println!("Counter match expected, e.g. 'tx_bytes' or 'tx_*'.");
                std::process::exit(1);
            }
        }
        rules = _rules;
    }

    let cycle_ms = 500;
    let avg_s = 5;
    let history_depth = 1000 * avg_s / cycle_ms;
    let avg_s_str = &format!("{}s avg", avg_s);

    let mut state = Vec::<CounterHistory>::new();
    loop {
        let start = std::time::Instant::now();

        // Trim the history at history_depth.
        for entry in &mut state {
            entry.age += 1;
            if entry.age >= history_depth {
                entry.history.remove(0);
            }
        }

        // Counters that disappeared (e.g. due to their interface having disappeared) will
        // eventually run out of history items. Drop them.
        state = state
            .drain(..)
            .filter(|entry| !entry.history.is_empty())
            .collect();

        for ifname in netlink::ifnames()
            .iter()
            .filter(|ifname| ifmatch.matches(&ifname))
        {
            for stat in ethtool_ss::stats_for(&ifname).iter() {
                for rule in &rules {
                    if !rule.pat.matches(&stat.name) {
                        continue;
                    }
                    let key = CounterKey {
                        ifname: ifname.clone(),
                        ctns: "ethtool".to_string(),
                        ctname: stat.name.clone(),
                    };
                    if let Some(elem) = state.iter_mut().find(|hist| hist.key == key) {
                        elem.history.push(stat.value);
                    } else {
                        state.push(CounterHistory {
                            key: key,
                            history: vec![stat.value],
                            base: stat.value,
                            age: 0,
                            unit: rule.unit.as_ref().unwrap().clone(),
                        });
                    }
                    break;
                }
            }
        }

        print!("{}", termion::clear::All);
        let mut line = 1;

        let headers = vec!["iface", "counter", "value", avg_s_str];

        let ifname_col_w = state
            .iter()
            .map(|entry| entry.key.ifname.len())
            .chain(vec![headers[0].len()].drain(..))
            .max()
            .unwrap();
        let ctname_col_w = state
            .iter()
            .map(|entry| entry.key.ctname.len())
            .chain(vec![headers[1].len()].drain(..))
            .max()
            .unwrap();

        let mut line_out = |ifname: &str, ctname: &str, value: &str, avg: &str| {
            print!(
                "{}{}| {: <ifname_col_w$} | {: <ctname_col_w$} | {: >14} | {: >14} |",
                termion::cursor::Goto(1, line as u16),
                termion::clear::CurrentLine,
                ifname,
                ctname,
                value,
                avg,
                ifname_col_w = ifname_col_w,
                ctname_col_w = ctname_col_w
            );
            line += 1;
        };

        print!("{}{}", termion::style::Invert, termion::style::Bold);
        line_out(headers[0], headers[1], headers[2], headers[3]);
        print!("{}", termion::style::Reset);

        let mut last_ifname = "";
        for entry in &state {
            let mi = *entry.history.first().unwrap();
            let ma = *entry.history.last().unwrap();
            let ma1 = if entry.history.len() > 1 {
                entry.history[entry.history.len() - 2]
            } else {
                ma
            };

            let mut avg = None;
            let mut value = match entry.unit.freq {
                UnitFrequency::AsIs => CtVal::from_num(ma),
                UnitFrequency::Delta => CtVal::from_num((ma - entry.base) as i64),
                UnitFrequency::PerSecond => {
                    if entry.history.len() == history_depth as usize {
                        let d1 = ma - mi;
                        avg = Some(CtVal::from_num(d1) / CtVal::from_num(avg_s));
                    }
                    CtVal::from_num(1000 * (ma - ma1) / (cycle_ms as u64))
                }
            };

            let mut prev_unit = None;
            let mut prefix = None;
            for unit in &entry.unit.units {
                match (prev_unit, unit.unit) {
                    (None, _) => {}
                    (Some(UnitBase::Bytes), UnitBase::Bits) => {
                        value *= 8;
                        avg = avg.map(|avalue| avalue * 8);
                    }
                    (Some(UnitBase::Bits), UnitBase::Bytes) => {
                        value /= 8;
                        avg = avg.map(|avalue| avalue / 8);
                    }
                    _ => {}
                }
                if prefix.is_none() {
                    prefix = Some(unit.prefix);
                }
                prev_unit = Some(unit.unit);
            }

            let unit_str = UNITS
                .iter()
                .find_map(|&(unit, letter)| {
                    if unit == prev_unit.unwrap() {
                        Some(letter)
                    } else {
                        None
                    }
                })
                .unwrap()
                .to_string()
                + match entry.unit.freq {
                    UnitFrequency::AsIs => "  ",
                    UnitFrequency::Delta => "  ",
                    UnitFrequency::PerSecond => "ps",
                };
            let unit_prefix_str = match entry.unit.freq {
                UnitFrequency::AsIs => " ",
                UnitFrequency::Delta => "\u{0394}",
                UnitFrequency::PerSecond => " ",
            };

            line_out(
                if last_ifname != entry.key.ifname {
                    &entry.key.ifname
                } else {
                    ""
                },
                &entry.key.ctname,
                &humanize(value, prefix.unwrap(), &unit_prefix_str, &unit_str),
                &if avg.is_some() {
                    humanize(avg.unwrap(), prefix.unwrap(), &unit_prefix_str, &unit_str)
                } else {
                    "-     ".to_string()
                },
            );
            last_ifname = &entry.key.ifname;
        }
        print!("\nOverhead {:?}", start.elapsed());
        stdout().flush().unwrap();

        thread::sleep(time::Duration::from_millis(cycle_ms as u64) - start.elapsed());
    }
}
