extern crate glob;
extern crate termion;

use std::env;
use std::io::{stdout, Write};
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
    age: u32,
}

fn main() {
    let ifmatch: glob::Pattern;
    let ctmatch: Vec<glob::Pattern>;
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

        let mut _ctmatch = Vec::<glob::Pattern>::new();
        for arg in args {
            if let Ok(pat) = glob::Pattern::new(&arg) {
                _ctmatch.push(pat);
            } else {
                println!("Counter match expected, e.g. 'tx_bytes' or 'tx_*'.");
                std::process::exit(1);
            }
        }
        ctmatch = _ctmatch;
    }

    let mut state = Vec::<CounterHistory>::new();
    loop {
        // Trim the history at 10 elements.
        for entry in &mut state {
            entry.age += 1;
            if entry.age >= 10 {
                entry.history.remove(0);
            }
        }

        // Counters that disappeared (e.g. due to their interface having disappeared) will
        // eventually run out of history items. Drop them.
        state = state
            .drain(..)
            .filter(|entry| !entry.history.is_empty())
            .collect();

        for ifname in netlink::ifnames() {
            if !ifmatch.matches(&ifname) {
                continue;
            }

            for stat in ethtool_ss::stats_for(&ifname)
                .iter()
                .filter(|ref stat| ctmatch.iter().any(|ref pat| pat.matches(&stat.name)))
            {
                let key = CounterKey {
                    ifname: ifname.clone(),
                    ctns: "ethtool".to_string(),
                    ctname: stat.name.clone(),
                };
                if let Some(pos) = state.iter().position(|ref hist| hist.key == key) {
                    state[pos].history.push(stat.value);
                } else {
                    state.push(CounterHistory {
                        age: 0,
                        key: key,
                        history: vec![stat.value],
                    });
                }
            }
        }

        print!("{}", termion::clear::All);
        let mut line = 1;

        for entry in &state {
            print!(
                "{}{}|{}\t|{}\t|{}",
                termion::cursor::Goto(1, line as u16),
                termion::clear::CurrentLine,
                entry.key.ifname,
                entry.key.ctname,
                entry.history.last().unwrap()
            );
            line += 1;
        }
        stdout().flush().unwrap();

        thread::sleep(time::Duration::from_millis(500));
    }
}
