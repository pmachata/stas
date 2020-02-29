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
    filter: Vec<Box<dyn stas::CounterValueFilter>>,
}

fn show_help_exit(rc: i32) {
    println!("Usage: stas [ethtool:] @eth* @ens* [...] tx_* rx_* /B/bps");
    std::process::exit(rc);
}

struct CounterLine<'a> {
    key: &'a stas::CounterKey,
    value: Option<stas::Value>,
    avg: Option<stas::Value>,
    freq: &'a stas::UFreq,
    unit: stas::Unit,
    filter: &'a Vec<Box<dyn stas::CounterValueFilter>>,
}

trait CounterListFilter {
    fn filter<'a>(&self, counters: Vec<CounterLine<'a>>) -> Vec<CounterLine<'a>>;
}

impl CounterListFilter for stas::NonZeroCounterFilter {
    fn filter<'a>(&self, mut counters: Vec<CounterLine<'a>>) -> Vec<CounterLine<'a>> {
        counters
            .drain(..)
            .filter(|cl| self.do_filter(&cl.value, &cl.avg))
            .collect()
    }
}

#[derive(Clone)]
struct ApplyValueFilters {}

impl CounterListFilter for ApplyValueFilters {
    fn filter<'a>(&self, mut counters: Vec<CounterLine<'a>>) -> Vec<CounterLine<'a>> {
        counters
            .drain(..)
            .filter(|cl| cl.filter.iter().all(|vf| vf.filter(&cl.value, &cl.avg)))
            .collect()
    }
}

fn main() {
    let mut list_filters: Vec<Box<dyn CounterListFilter>> = Vec::new();
    list_filters.push(Box::new(ApplyValueFilters {}));
    let mut once: bool = false;
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
                    list_filters.push(Box::new(stas::NonZeroCounterFilter {}));
                    it.next();
                }
                "--once" => {
                    once = true;
                    it.next();
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
                                filter: imm.filter.iter().map(|vf| vf.clone_box()).collect(),
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
                key: &entry.key,
                value: value,
                avg: avg,
                freq: &entry.unit.freq,
                unit: unit,
                filter: &entry.filter,
            });
        }

        for cf in &list_filters {
            counter_lines = cf.filter(counter_lines);
        }

        print!("{}", termion::cursor::Goto(1, 1));
        let mut line = 1;

        struct Column {
            width: usize,
            last: Option<String>,
        }
        let mut columns = std::collections::HashMap::new();
        for entry in &state {
            for (head, value) in &entry.key.key {
                let column = columns.entry(head).or_insert(Column {
                    width: head.column_head().len(),
                    last: None,
                });
                column.width = std::cmp::max(column.width, value.len());
            }
        }

        let headers = &stas::ALL_HEADS
            .iter()
            .filter(|head| columns.contains_key(head))
            .map(|head| (*head, head.column_head().to_string()))
            .collect();
        let mut line_out =
            |key: &Vec<(stas::KeyHead, String)>, value: &str, avg: &str, is_value: bool| {
                print!(
                    "{}{}",
                    termion::cursor::Goto(1, line as u16),
                    termion::clear::CurrentLine
                );
                let unused_head = "-".to_string();
                for head in &stas::ALL_HEADS {
                    if columns.contains_key(head) {
                        let value = key
                            .iter()
                            .find(|(h, _v)| h == head)
                            .map(|(_h, v)| v)
                            .unwrap_or(&unused_head);
                        let mut show = value.as_str();
                        if is_value && head.suppress_dups() {
                            if let Some(ref last) = &columns[head].last {
                                if last == value {
                                    show = "";
                                }
                            }
                            columns.get_mut(head).unwrap().last = Some(value.to_string());
                        }
                        print!(
                            "{} {: <w$} ",
                            if head.separate() { "|" } else { "" },
                            show,
                            w = columns[head].width,
                        );
                    }
                }
                print!("| {: >14} | {: >14} |", value, avg,);
                line += 1;
            };

        print!("{}{}", termion::style::Invert, termion::style::Bold);
        line_out(&headers, "value", avg_s_str, false);
        print!("{}", termion::style::Reset);

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
                &counter_line.key.key,
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
                true,
            );
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

        if once {
            break;
        }

        let cycle_dur = time::Duration::from_millis(cycle_ms as u64);
        let e_dur = start.elapsed();
        if cycle_dur > e_dur {
            thread::sleep(cycle_dur - e_dur);
        }
    }
}
