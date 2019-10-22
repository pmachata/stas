extern crate neli;

use neli::consts::*;
use neli::nl::Nlmsghdr;
use neli::rtnl::*;
use neli::socket::*;
//use neli::err::*;

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

pub fn ifnames() -> Vec<String> {
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
}
