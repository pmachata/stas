extern crate alloc;
extern crate libc;
extern crate nix;

use std::ffi::CStr;
use std::mem;
use std::os::unix::io::RawFd;

use libc::{c_char, c_void};

use nix::sys::socket;
use nix::sys::socket::{AddressFamily, SockFlag, SockType};
use nix::unistd::close;

fn open_sock() -> RawFd {
    match socket::socket(
        AddressFamily::Inet,
        SockType::Datagram,
        SockFlag::empty(),
        None,
    ) {
        Ok(raw_fd) => raw_fd,
        Err(e) => {
            println!("Failed to open socket: {}", e);
            std::process::exit(1);
        }
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

    let err = unsafe { libc::ioctl(fd, SIOCETHTOOL, &ifr) };
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
        let ptr: *mut u8 = alloc::alloc::alloc_zeroed(layout);
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
        let ptr: *mut u8 = alloc::alloc::alloc_zeroed(layout);
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

pub struct Stat {
    pub name: String,
    pub value: u64,
}

pub fn stats_for(ifname: &String) -> Vec<Stat> {
    let fd = open_sock();
    let len = ethtool_ss_stats_len(fd, &ifname);
    let mut statnames = ethtool_ss_stats_names(fd, &ifname, len);
    let mut statvalues = ethtool_ss_stats_values(fd, &ifname, len);
    close(fd).unwrap();

    statnames
        .drain(..)
        .zip(statvalues.drain(..))
        .map(|(name, value)| Stat { name, value })
        .collect()
}
