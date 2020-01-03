extern crate glob;
extern crate termion;

use ::stas;

use std::env;
use std::io::{stdout, Write};
use std::thread;
use std::time;

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
    unit: stas::UnitChain,
}

fn main() {
    let ifmatch: glob::Pattern;
    let rules: Vec<stas::CounterRule>;

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

        let mut _rules = Vec::<stas::CounterRule>::new();
        for arg in args {
            match stas::parse_unit(&arg) {
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
                _rules.push(stas::CounterRule {
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

        for ifname in stas::ifnames()
            .iter()
            .filter(|ifname| ifmatch.matches(&ifname))
        {
            for stat in stas::stats_for(&ifname).iter() {
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
            let value = match entry.unit.freq {
                stas::UFreq::AsIs => stas::Value::from_num(ma),
                stas::UFreq::Delta => stas::Value::from_num((ma - entry.base) as i64),
                stas::UFreq::PerSecond => {
                    if entry.history.len() == history_depth as usize {
                        let d1 = ma - mi;
                        avg = Some(stas::Value::from_num(d1) / stas::Value::from_num(avg_s));
                    }
                    stas::Value::from_num(1000 * (ma - ma1) / (cycle_ms as u64))
                }
            };

            let (value, avg, unit) = stas::convert(&entry.unit, value, avg);

            let unit_str = unit.base.to_string()
                + match entry.unit.freq {
                    stas::UFreq::AsIs => "  ",
                    stas::UFreq::Delta => "  ",
                    stas::UFreq::PerSecond => "ps",
                };
            let unit_prefix_str = match entry.unit.freq {
                stas::UFreq::AsIs => " ",
                stas::UFreq::Delta => "\u{0394}",
                stas::UFreq::PerSecond => " ",
            };

            line_out(
                if last_ifname != entry.key.ifname {
                    &entry.key.ifname
                } else {
                    ""
                },
                &entry.key.ctname,
                &stas::humanize(value, unit.prefix, &unit_prefix_str, &unit_str),
                &if avg.is_some() {
                    stas::humanize(avg.unwrap(), unit.prefix, &unit_prefix_str, &unit_str)
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
