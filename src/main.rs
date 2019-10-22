extern crate neli;
extern crate glob;
extern crate termion;

use std::env;
use std::io::{stdout, Write};
use std::thread;
use std::time;

use neli::consts::*;
use neli::nl::Nlmsghdr;
use neli::rtnl::*;
use neli::socket::*;
//use neli::err::*;

mod ethtool_ss;

fn ifla_link_name(ifi: Nlmsghdr<u16, Ifinfomsg<Ifla>>) -> Option<String> {
    for attr in ifi.nl_payload.rtattrs {
        let payload: Vec<u8> = attr.rta_payload;
        match attr.rta_type {
            Ifla::Ifname => {
                // Snip terminating zero.
                let prefix = &payload[.. (payload.len() - 1)];
                return Some(String::from_utf8_lossy(prefix).into_owned())
            },
            _ => break,
        }
    }

    None
}

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

    let ifnames = {
        let mut socket = NlSocket::connect(NlFamily::Route, None, None, true).unwrap();
        let ifim: Ifinfomsg<Ifla> = {
            let ifi_family = RtAddrFamily::from(0);
            let ifi_type = Arphrd::Ether;
            let ifi_index = 0;
            let ifi_flags = vec![];
            let rtattrs = vec![];
            Ifinfomsg::new(ifi_family, ifi_type, ifi_index, ifi_flags, rtattrs)
        };
        let nlhdr = {
            let len = None;
            let nl_type = Rtm::Getlink;
            let flags = vec![NlmF::Request, NlmF::Dump];
            let seq = None;
            let pid = None;
            let payload = ifim;
            Nlmsghdr::new(len, nl_type, flags, seq, pid, payload)
        };

        socket.send_nl(nlhdr).unwrap();

        let mut ifnames = Vec::<String>::new();
        while let Ok(nl) = socket.recv_nl::<u16, Ifinfomsg<Ifla>>(None) {
            if let Some(ifname) = ifla_link_name(nl) {
                ifnames.push(ifname);
            }
        }
        ifnames
    };

    loop {
        print!("{}", termion::clear::All);
        let mut line = 1;

        for ifname in &ifnames {
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
