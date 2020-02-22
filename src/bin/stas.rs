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
    curr: Option<u64>,
    prev: Option<u64>,
    base: u64,
    age: u32,
    unit: stas::UnitChain,
}

fn show_help_exit(rc: i32) {
    println!("Usage: stas [ethtool:] @eth* @ens* [...] tx_* rx_* /B/bps");
    std::process::exit(rc);
}

struct CounterLine<'a> {
    ifname: &'a str,
    ctname: &'a str,
    value: Option<stas::Value>,
    avg: Option<stas::Value>,
    freq: &'a stas::UFreq,
    unit: stas::Unit,
}

trait CounterFilter {
    fn filter<'a>(&self, counters: Vec<CounterLine<'a>>) -> Vec<CounterLine<'a>>;
}

struct NonZeroCounterFilter {}

impl CounterFilter for NonZeroCounterFilter {
    fn filter<'a>(&self, mut counters: Vec<CounterLine<'a>>) -> Vec<CounterLine<'a>> {
        counters
            .drain(..)
            .filter(|cl| {
                cl.value.unwrap_or(stas::Value::from_num(0)) != 0
                    || cl.avg.unwrap_or(stas::Value::from_num(0)) != 0
            })
            .collect()
    }
}

fn main() {
    let mut counter_filters: Vec<Box<dyn CounterFilter>> = Vec::new();
    let rules;

    {
        let mut args: Vec<String> = env::args().collect();
        if args.len() <= 1 {
            show_help_exit(1);
        }
        args.remove(0);

        let mut it = args.iter().peekable();
        while let Some(arg) = it.peek() {
            match &arg[..] {
                "--non0" => {
                    counter_filters.push(Box::new(NonZeroCounterFilter {}));
                    it.next();
                }
                "--once" => {
                    println!("once");
                    return;
                    // xxx
                    // it.next();
                }
                "--help" => {
                    show_help_exit(0);
                }
                _ => {
                    if arg.starts_with("-") {
                        println!("Invalid argument {}", arg);
                        show_help_exit(1);
                    }
                    break;
                }
            }
        }

        match stas::parse_expr(&mut it) {
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

    //print!("{}", termion::clear::All);
    let mut state = Vec::<CounterHistory>::new();
    let mut nlines = 0;
    loop {
        let start = std::time::Instant::now();

        // Trim the history at history_depth.
        for entry in &mut state {
            entry.age += 1;
            if let Some(prev) = entry.prev {
                entry.history.push(prev);
            }
            if !entry.history.is_empty() && entry.age > history_depth {
                entry.history.remove(0);
            }
            entry.prev = entry.curr;
            entry.curr = None;
        }

        // Counters that disappeared (e.g. due to their interface having disappeared) will
        // eventually run out of history items. Drop them.
        state = state
            .drain(..)
            .filter(|entry| !(entry.history.is_empty() && entry.prev.is_none()))
            .collect();

        for rule in &rules {
            match rule.counters() {
                Ok(imms) => {
                    for imm in imms {
                        if let Some(elem) = state.iter_mut().find(|hist| hist.key == imm.key) {
                            elem.curr = Some(imm.value);
                        } else {
                            state.push(CounterHistory {
                                key: imm.key,
                                history: vec![],
                                base: imm.value,
                                curr: Some(imm.value),
                                prev: None,
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

        let mut counter_lines: Vec<CounterLine> = Vec::new();
        for entry in &state {
            // -1 for the first tick, which does not go into history.
            let avg =
                if (entry.history.len() >= (history_depth - 1) as usize) && entry.curr.is_some() {
                    let mi = *entry.history.first().unwrap();
                    let ma = entry.curr.unwrap();
                    let d1 = stas::Value::from_num(ma) - stas::Value::from_num(mi);
                    Some(d1 / stas::Value::from_num(avg_s))
                } else {
                    None
                };

            let value = match entry.unit.freq {
                stas::UFreq::AsIs => entry.curr.map(|v| stas::Value::from_num(v)),
                stas::UFreq::Delta => entry
                    .curr
                    .map(|v| stas::Value::from_num(v) - stas::Value::from_num(entry.base)),
                stas::UFreq::PerSecond => match (entry.prev, entry.curr) {
                    (Some(prev), Some(curr)) => Some(
                        1000 * (stas::Value::from_num(curr) - stas::Value::from_num(prev))
                            / stas::Value::from_num(cycle_ms),
                    ),
                    (_, _) => None,
                },
            };

            let (value, avg, unit) = stas::convert(&entry.unit, value, avg);
            counter_lines.push(CounterLine {
                ifname: &entry.key.ifname,
                ctname: &entry.key.ctname,
                value: value,
                avg: avg,
                freq: &entry.unit.freq,
                unit: unit,
            });
        }

        for cf in &counter_filters {
            counter_lines = cf.filter(counter_lines);
        }

        print!("{}", termion::cursor::Goto(1, 1));
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
        for counter_line in &counter_lines {
            let unit_str = counter_line.unit.base.to_string()
                + match counter_line.freq {
                    stas::UFreq::AsIs => "  ",
                    stas::UFreq::Delta => "  ",
                    stas::UFreq::PerSecond => "ps",
                };
            let unit_prefix_str = match counter_line.freq {
                stas::UFreq::AsIs => " ",
                stas::UFreq::Delta => "\u{0394}",
                stas::UFreq::PerSecond => " ",
            };

            line_out(
                if last_ifname != counter_line.ifname {
                    &counter_line.ifname
                } else {
                    ""
                },
                &counter_line.ctname,
                &if counter_line.value.is_some() {
                    stas::humanize(
                        counter_line.value.unwrap(),
                        counter_line.unit.prefix,
                        &unit_prefix_str,
                        &unit_str,
                        false,
                    )
                } else {
                    "-     ".to_string()
                },
                &if counter_line.avg.is_some() {
                    stas::humanize(
                        counter_line.avg.unwrap(),
                        counter_line.unit.prefix,
                        &unit_prefix_str,
                        &unit_str,
                        true,
                    )
                } else {
                    "-     ".to_string()
                },
            );
            last_ifname = &counter_line.ifname;
        }

        print!(
            "\n{}Overhead {:?}",
            termion::clear::CurrentLine,
            start.elapsed()
        );
        for _ in line..nlines {
            print!("\n{}", termion::clear::CurrentLine);
        }
        nlines = line;
        stdout().flush().unwrap();

        thread::sleep(time::Duration::from_millis(cycle_ms as u64) - start.elapsed());
    }
}
