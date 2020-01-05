extern crate glob;
extern crate termion;

use ::stas;

use std::env;
use std::io::{stdout, Write};
use std::thread;
use std::time;

struct CounterHistory {
    key: stas::CounterKey,
    history: Vec<u64>,
    base: u64,
    age: u32,
    unit: stas::UnitChain,
}

fn main() {
    let rules;

    {
        let mut args: Vec<String> = env::args().collect();
        if args.len() <= 2 {
            println!("Usage: {} <if> <counter> [<counter> ...]", args[0]);
            std::process::exit(1);
        }
        args.remove(0);

        match stas::parse_expr(&mut args.iter().peekable()) {
            Ok(r) => rules = r,
            Err(e) => {
                println!("Error: {}", e);
                std::process::exit(1);
            }
        }
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

        for rule in &rules {
            match rule.counters() {
                Ok(imms) => {
                    for imm in imms {
                        if let Some(elem) = state.iter_mut().find(|hist| hist.key == imm.key) {
                            elem.history.push(imm.value);
                        } else {
                            state.push(CounterHistory {
                                key: imm.key,
                                history: vec![imm.value],
                                base: imm.value,
                                age: 0,
                                unit: imm.unit,
                            });
                        }
                    }
                }
                Err(err) => {
                    println!("Error when obtaining counter values: {}", err);
                    return;
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
