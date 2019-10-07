extern crate alloc;
extern crate libc;
extern crate neli;
extern crate nix;
extern crate glob;
extern crate termion;

use std::env;
use std::ffi::CStr;
use std::io::{stdout, Write};
use std::mem;
use std::os::unix::io::RawFd;
use std::thread;
use std::time;

use libc::{c_void, c_char};

use nix::sys::socket;
//use nix::unistd::close;
use nix::sys::socket::{AddressFamily, SockType, SockFlag};

use neli::consts::*;
use neli::nl::Nlmsghdr;
use neli::rtnl::*;
use neli::socket::*;
//use neli::err::*;

fn open_sock() -> RawFd {
    match socket::socket(AddressFamily::Inet, SockType::Datagram, SockFlag::empty(), None) {
        Ok(raw_fd) => raw_fd,
        Err(e) => {
            println!("Failed to open socket: {}", e);
            std::process::exit(1);
        },
    }
}

const SIOCETHTOOL: libc::c_ulong = 0x8946;

const ETHTOOL_GSSET_INFO: u32 = 0x00000037;
const ETHTOOL_GSTRINGS: u32 = 0x0000001b;
const ETHTOOL_GSTATS: u32 = 0x0000001d;

const ETH_SS_STATS: u8 = 1;

const IFNAMSIZ: usize = 16;
const ETH_GSTRING_LEN: usize = 32;

#[repr(C)]
struct ethtool_sset_info {
    cmd: u32,
    reserved: u32,
    sset_mask: u64,
    length: u32,
}

#[repr(C)]
struct ethtool_gstrings {
    cmd: u32,
    string_set: u32,
    len: u32,
}

#[repr(C)]
struct ethtool_stats {
    cmd: u32,
    n_stats: u32,
}

#[repr(C)]
struct ifreq {
    ifr_name: [u8; IFNAMSIZ],
    ifr_data: *mut c_void,
}

fn ethtool_ioctl(fd: RawFd, ifname: &String, data: *mut c_void) {
    let ifr = ifreq {
        ifr_name: {
            let mut buf = [0u8; IFNAMSIZ];
            buf[..ifname.len()].copy_from_slice(ifname.as_bytes());
            buf
        },
        ifr_data: data,
    };

    let err = unsafe {libc::ioctl(fd, SIOCETHTOOL, &ifr)};
    if err != 0 {
        println!("ioctl SIOCETHTOOL failed: errno={}", err);
        std::process::exit(1);
    }
}

fn ethtool_ss_stats_len(fd: RawFd, ifname: &String) -> u32 {
    let mut sset_info = ethtool_sset_info {
        cmd: ETHTOOL_GSSET_INFO,
        reserved: 0,
        sset_mask: 1u64 << ETH_SS_STATS,
        length: 0,
    };

    ethtool_ioctl(fd, &ifname, &mut sset_info as *mut _ as *mut c_void);

    sset_info.length
}

fn ethtool_ss_stats_names(fd: RawFd, ifname: &String, len: u32) -> Vec<String> {
    if len == 0 {
        return Vec::<String>::new();
    }

    let gsz = mem::size_of::<ethtool_gstrings>();
    let sz = gsz + len as usize * ETH_GSTRING_LEN;
    let gal = mem::align_of::<ethtool_gstrings>();
    let layout = alloc::alloc::Layout::from_size_align(sz, gal).unwrap();

    let strings: &mut ethtool_gstrings = unsafe {
        let ptr: *mut u8  = alloc::alloc::alloc_zeroed(layout);
        if ptr.is_null() {
            alloc::alloc::handle_alloc_error(layout);
        }
        &mut *(ptr as *mut _)
    };

    *strings = ethtool_gstrings {
        cmd: ETHTOOL_GSTRINGS,
        string_set: ETH_SS_STATS as u32,
        len: len,
    };

    ethtool_ioctl(fd, &ifname, strings as *mut _ as *mut c_void);

    let mut statnames = Vec::<String>::new();
    unsafe {
        let mut ptr = strings as *const _ as *const c_char;
        ptr = ptr.add(gsz);

        for _ in 0..len {
            let statname = CStr::from_ptr(ptr).to_string_lossy().into_owned();
            statnames.push(statname);
            ptr = ptr.add(ETH_GSTRING_LEN);
        }
    }

    unsafe {
        alloc::alloc::dealloc(strings as *mut _ as *mut u8, layout);
    }

    statnames
}

fn ethtool_ss_stats_values(fd: RawFd, ifname: &String, len: u32) -> Vec<u64> {
    if len == 0 {
        return Vec::<u64>::new();
    }

    let gsz = mem::size_of::<ethtool_stats>();
    let sz = gsz + len as usize * 8;
    let layout = alloc::alloc::Layout::from_size_align(sz, 8).unwrap();

    let stats: &mut ethtool_stats = unsafe {
        let ptr: *mut u8  = alloc::alloc::alloc_zeroed(layout);
        if ptr.is_null() {
            alloc::alloc::handle_alloc_error(layout);
        }
        &mut *(ptr as *mut _)
    };

    *stats = ethtool_stats {
        cmd: ETHTOOL_GSTATS,
        n_stats: len,
    };

    ethtool_ioctl(fd, &ifname, stats as *mut _ as *mut c_void);

    let mut statvalues = Vec::<u64>::new();
    unsafe {
        let mut ptr = stats as *const _ as *const u64;
        ptr = ptr.add(1); // skip ethtool_stats header, 2*u32 = 1*u64

        for _ in 0..len {
            statvalues.push(*ptr);
            ptr = ptr.add(1);
        }
    }

    unsafe {
        alloc::alloc::dealloc(stats as *mut _ as *mut u8, layout);
    }

    statvalues
}

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
            let fd = open_sock();
            let len = ethtool_ss_stats_len(fd, &ifname);
            let statnames = ethtool_ss_stats_names(fd, &ifname, len);

            let statvalues = ethtool_ss_stats_values(fd, &ifname, len);
            for (name, value) in statnames.iter().zip(statvalues.iter()) {
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

    //close(fd).unwrap();
}
