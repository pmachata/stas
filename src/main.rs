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

struct CounterHistory {
    key: CounterKey,
    history: Vec<u64>,
    base: u64,
    age: u32,
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

fn parse_unit_one<I>(_it: &mut Peekable<I>) -> Result<(UnitPrefix, UnitFrequency), String>
where
    I: Iterator<Item = char>,
{
    Ok((
        UnitPrefix {
            power: 0,
            unit: UnitBase::Bytes,
        },
        UnitFrequency::PerSecond,
    ))
}

fn parse_unit_chain<I>(mut it: Peekable<I>) -> Result<UnitChain, String>
where
    I: Iterator<Item = char>,
{
    let mut units = Vec::<UnitPrefix>::new();
    let mut freq = UnitFrequency::AsIs;

    while let Some(c) = it.next() {
        if c == '/' {
            let (unit, this_freq) = parse_unit_one(&mut it)?;
            if this_freq != UnitFrequency::AsIs {
                if freq != UnitFrequency::AsIs {
                    return Err("Only one frequency allowed in a unit chain.".to_string());
                }
                freq = this_freq;
            }
            units.push(unit);
        } else {
            return Err(format!("Expected '/' to separate units, got '{}'.", c));
        }
    }

    Ok(UnitChain {
        units: units,
        freq: freq,
    })
}

fn parse_unit(str: &String) -> Result<Option<UnitChain>, String> {
    let mut it = str.chars().peekable();
    match it.peek() {
        Some('/') => Ok(Some(parse_unit_chain(it)?)),
        _ => Ok(None),
    }
}

struct CounterRule {
    pat: glob::Pattern,
    unit: Option<UnitChain>,
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

        let arg1 = args.remove(1);
        if let Ok(pat) = glob::Pattern::new(&arg1) {
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
            for stat in ethtool_ss::stats_for(&ifname)
                .iter()
                .filter(|stat| rules.iter().any(|rule| rule.pat.matches(&stat.name)))
            {
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
                    });
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

            line_out(
                if last_ifname != entry.key.ifname {
                    &entry.key.ifname
                } else {
                    ""
                },
                &entry.key.ctname,
                &humanize(delta, " "),
                &humanize(spot, " "),
                &humanize(avg, " "),
            );
            last_ifname = &entry.key.ifname;
        }
        print!("\nOverhead {:?}", start.elapsed());
        stdout().flush().unwrap();

        thread::sleep(time::Duration::from_millis(cycle_ms as u64) - start.elapsed());
    }
}
