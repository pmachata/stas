extern crate glob;
extern crate termion;

use std::env;
use std::io::{stdout, Write};
use std::thread;
use std::time;

mod ethtool_ss;
mod netlink;

fn main() {
    let ifmatch = {
        let mut args: Vec<String> = env::args().collect();
        if args.len() != 2 {
            println!("Usage: {} <if>", args[0]);
            std::process::exit(1);
        }

        let arg1 = args.remove(1);
        if let Ok(ifmatch) = glob::Pattern::new(&arg1) {
            ifmatch
        } else {
            println!("Interface match expected, e.g. 'eth0' or 'eth*'.");
            std::process::exit(1);
        }
    };

    loop {
        print!("{}", termion::clear::All);
        let mut line = 1;

        for ifname in netlink::ifnames() {
            if !ifmatch.matches(&ifname) {
                continue;
            }

            println!("== {} ==", ifname);
            line += 1;
            for ethtool_ss::Stat{name, value} in ethtool_ss::stats_for(&ifname) {
                print!("{}{}{}\t{}",
                       termion::cursor::Goto(1, line as u16),
                       termion::clear::CurrentLine,
                       name, value);
                line += 1;
            }
        }

        stdout().flush().unwrap();
        thread::sleep(time::Duration::from_millis(200));
    }
}
