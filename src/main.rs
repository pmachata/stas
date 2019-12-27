extern crate glob;
extern crate termion;

use std::env;
use std::io::{stdout, Write};
use std::iter::Peekable;
use std::thread;
use std::time;

mod ethtool_ss;
mod netlink;

#[derive(Eq, PartialEq)]
struct CounterKey {
    ifname: String,
    ctns: String,
    ctname: String,
}

fn humanize(value: i32, base: &str) -> String {
    let units = vec!["p", "n", "u", "m", " ", "K", "M", "G", "T", "P"];
    let mut unit = units.iter().position(|u| *u == base).unwrap();
    let mut f = (value as f32).abs();
    let mut trivial = true;

    while f > 1000.0 && unit < units.len() {
        f /= 1000.0;
        unit += 1;
        trivial = false;
    }

    if trivial {
        format!(
            "{}{:.0}   {}",
            if value < 0 { "-" } else { "" },
            f,
            units[unit]
        )
    } else {
        format!(
            "{}{:.2}{}",
            if value < 0 { "-" } else { "" },
            f,
            units[unit]
        )
    }
}

#[derive(Clone)]
enum UnitBase {
    Units,
    Packets,
    Seconds,
    Bits,
    Bytes,
}

#[derive(Clone)]
struct UnitPrefix {
    power: i32,
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
    units: Vec<UnitPrefix>,
    freq: UnitFrequency,
}

fn parse_unit_pfx<I>(it: &mut Peekable<I>) -> Result<UnitPrefix, String>
where
    I: Iterator<Item = char>,
{
    let power = match it.peek() {
        Some(&'G') => {
            it.next();
            9
        }
        Some(&'M') => {
            it.next();
            6
        }
        Some(&'k') | Some(&'K') => {
            it.next();
            3
        }
        Some(&'m') => {
            it.next();
            -3
        }
        Some(&'u') => {
            it.next();
            -6
        }
        Some(&'n') => {
            it.next();
            -9
        }
        _ => 0,
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

    Ok(UnitPrefix {
        power: power,
        unit: unit,
    })
}

fn parse_unit_freq(str: &str) -> Result<(UnitPrefix, UnitFrequency), String> {
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
    let mut units = Vec::<UnitPrefix>::new();
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

        let headers = vec!["iface", "counter", "delta", "spot", avg_s_str];

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

        let mut line_out = |ifname: &str, ctname: &str, delta: &str, spot: &str, avg: &str| {
            print!(
                "{}{}| {: <ifname_col_w$} | {: <ctname_col_w$} | {: >10} | {: >10} | {: >10} |",
                termion::cursor::Goto(1, line as u16),
                termion::clear::CurrentLine,
                ifname,
                ctname,
                delta,
                spot,
                avg,
                ifname_col_w = ifname_col_w,
                ctname_col_w = ctname_col_w
            );
            line += 1;
        };

        print!("{}{}", termion::style::Invert, termion::style::Bold);
        line_out(headers[0], headers[1], headers[2], headers[3], headers[4]);
        print!("{}", termion::style::Reset);

        let mut last_ifname = "";
        for entry in &state {
            let mi = *entry.history.first().unwrap() as i64;
            let ma = *entry.history.last().unwrap() as i64;
            let ma1 = if entry.history.len() > 1 {
                entry.history[entry.history.len() - 2] as i64
            } else {
                ma
            };

            let time = entry.history.len();
            let d1 = (ma - mi) as f32;
            let avg = (d1 / (time as f32)) as i32;
            let spot = (ma - ma1) as i32;
            let delta = (ma - entry.base as i64) as i32;

            match entry.unit.freq {
                UnitFrequency::AsIs => {}
                UnitFrequency::Delta => {
                    line_out(
                        if last_ifname != entry.key.ifname {
                            &entry.key.ifname
                        } else {
                            ""
                        },
                        &entry.key.ctname,
                        &humanize(delta, " "),
                        "",
                        "",
                    );
                }
                UnitFrequency::PerSecond => {
                    line_out(
                        if last_ifname != entry.key.ifname {
                            &entry.key.ifname
                        } else {
                            ""
                        },
                        &entry.key.ctname,
                        "",
                        &humanize(spot, " "),
                        &humanize(avg, " "),
                    );
                }
            }
            last_ifname = &entry.key.ifname;
        }
        print!("\nOverhead {:?}", start.elapsed());
        stdout().flush().unwrap();

        thread::sleep(time::Duration::from_millis(cycle_ms as u64) - start.elapsed());
    }
}
