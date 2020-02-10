extern crate neli;

use neli::consts::*;
use neli::nl::Nlmsghdr;
use neli::rtnl::*;
use neli::socket::*;
//use neli::err::*;

fn ifla_link_name_index(ifi: Nlmsghdr<u16, Ifinfomsg>) -> Option<(libc::c_int, String)> {
    let index = ifi.nl_payload.ifi_index;
    for attr in ifi.nl_payload.rtattrs {
        let payload: Vec<u8> = attr.rta_payload;
        match attr.rta_type {
            Ifla::Ifname => {
                // Snip terminating zero.
                let prefix = &payload[..(payload.len() - 1)];
                return Some((index, String::from_utf8_lossy(prefix).into_owned()));
            }
            _ => break,
        }
    }

    None
}

fn ifindices_ifnames() -> Vec<(libc::c_int, String)> {
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

    let mut ifindices_ifnames = Vec::new();
    while let Ok(nl) = socket.recv_nl::<u16, Ifinfomsg>(None) {
        if let Some(ifname_index) = ifla_link_name_index(nl) {
            ifindices_ifnames.push(ifname_index);
        }
    }
    ifindices_ifnames
}

pub fn ifnames() -> Vec<String> {
    ifindices_ifnames()
        .drain(..)
        .map(|(_ifindex, name)| name)
        .collect()
}

pub fn ifindex_map() -> std::collections::HashMap<libc::c_int, String> {
    ifindices_ifnames().drain(..).collect()
}
