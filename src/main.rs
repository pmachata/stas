extern crate glob;
extern crate termion;

use std::env;
use std::io::{stdout, Write};
use std::thread;
use std::time;

mod ethtool_ss;
mod netlink;

fn main() {
    let ifmatch: glob::Pattern;
    let ctmatch: Vec::<glob::Pattern>;
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

    loop {
	print!("{}", termion::clear::All);
        let mut line = 1;
        for ifname in netlink::ifnames() {
            if !ifmatch.matches(&ifname) {
                continue;
            }

            for ethtool_ss::Stat{name, value} in ethtool_ss::stats_for(&ifname) {
		for ref pat in &ctmatch {
		    if !pat.matches(&name) {
			continue;
		    }
		    print!("{}{}|{}\t|{}\t|{}",
			   termion::cursor::Goto(1, line as u16),
			   termion::clear::CurrentLine,
			   ifname, name, value);
		    line += 1;
		}
	    }
	}

	stdout().flush().unwrap();
        thread::sleep(time::Duration::from_millis(200));
    }
}
