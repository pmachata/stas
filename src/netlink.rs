extern crate libc;
extern crate neli;

use neli::consts::*;
use neli::err::*;
use neli::impl_var;
use neli::impl_var_base;
use neli::nl::Nlmsghdr;
use neli::rtnl::*;
use neli::socket::*;
use neli::Nl;
use neli::StreamReadBuffer;
use neli::StreamWriteBuffer;

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
    let ifim: Ifinfomsg = {
        let ifi_family = RtAddrFamily::from(0);
        let ifi_type = Arphrd::Ether;
        let ifi_index = 0;
        let ifi_flags = vec![];
        let rtattrs = Rtattrs::empty();
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

neli::impl_var_trait!(
    /// Enum for use with `Rtattr.rta_type`.
    /// Values are nested attributes to TCA_STATS2
    TcaStats2, libc::c_ushort, RtaType,
    Unspec => 0 as libc::c_ushort,
    Basic => 1 as libc::c_ushort,
    RateEst => 2 as libc::c_ushort,
    Queue => 3 as libc::c_ushort,
    App => 4 as libc::c_ushort,
    RateEst64 => 5 as libc::c_ushort,
    Pad => 6 as libc::c_ushort
);

#[derive(Debug)]
struct GnetStatsBasic {
    bytes: u64,
    packets: u32,
}

impl Nl for GnetStatsBasic {
    fn serialize(&self, mem: &mut StreamWriteBuffer) -> Result<(), SerError> {
        self.bytes.serialize(mem)?;
        self.packets.serialize(mem)?;
        0u32.serialize(mem)?;
        Ok(())
    }

    fn deserialize<B>(mem: &mut StreamReadBuffer<B>) -> Result<Self, DeError>
    where
        B: AsRef<[u8]>,
    {
        let bytes = u64::deserialize(mem)?;
        let packets = u32::deserialize(mem)?;
        let pad = u32::deserialize(mem)?;
        if pad != 0 {
            return Err(DeError::new(
                "GnetStatsBasic expects 4 bytes of zero padding at the end of the structure",
            ));
        }
        Ok(GnetStatsBasic {
            bytes: bytes,
            packets: packets,
        })
    }

    fn size(&self) -> usize {
        self.bytes.size() + self.packets.size() + 0u32.size()
    }
}

#[derive(Debug)]
struct GnetStatsQueue {
    qlen: u32,
    backlog: u32,
    drops: u32,
    requeues: u32,
    overlimits: u32,
}

impl Nl for GnetStatsQueue {
    fn serialize(&self, mem: &mut StreamWriteBuffer) -> Result<(), SerError> {
        self.qlen.serialize(mem)?;
        self.backlog.serialize(mem)?;
        self.drops.serialize(mem)?;
        self.requeues.serialize(mem)?;
        self.overlimits.serialize(mem)?;
        Ok(())
    }

    fn deserialize<B>(mem: &mut StreamReadBuffer<B>) -> Result<Self, DeError>
    where
        B: AsRef<[u8]>,
    {
        Ok(GnetStatsQueue {
            qlen: u32::deserialize(mem)?,
            backlog: u32::deserialize(mem)?,
            drops: u32::deserialize(mem)?,
            requeues: u32::deserialize(mem)?,
            overlimits: u32::deserialize(mem)?,
        })
    }

    fn size(&self) -> usize {
        self.qlen.size()
            + self.backlog.size()
            + self.drops.size()
            + self.requeues.size()
            + self.overlimits.size()
    }
}

#[derive(Debug)]
struct GnetStatsRateEst<T> {
    bps: T,
    pps: T,
}

impl<T> Nl for GnetStatsRateEst<T>
where
    T: Nl,
{
    fn serialize(&self, mem: &mut StreamWriteBuffer) -> Result<(), SerError> {
        self.bps.serialize(mem)?;
        self.pps.serialize(mem)?;
        Ok(())
    }

    fn deserialize<B>(mem: &mut StreamReadBuffer<B>) -> Result<Self, DeError>
    where
        B: AsRef<[u8]>,
    {
        Ok(GnetStatsRateEst::<T> {
            bps: T::deserialize(mem)?,
            pps: T::deserialize(mem)?,
        })
    }

    fn size(&self) -> usize {
        self.bps.size() + self.pps.size()
    }
}

#[derive(Debug)]
pub struct QdiscStat {
    pub ifname: String,
    pub kind: String,
    pub handle: u32,
    pub parent: u32,
    pub name: String,
    pub value: u64,
}

pub fn qdiscs() -> Vec<QdiscStat> {
    let ifnames = ifindex_map();
    let mut ret = Vec::new();

    let mut socket = NlSocket::connect(NlFamily::Route, None, None, true).unwrap();
    let tcm = Tcmsg {
        tcm_family: 0,
        tcm_ifindex: 0,
        tcm_handle: 0,
        tcm_parent: 0,
        tcm_info: 0,
        rtattrs: Rtattrs::empty(),
    };
    let nlhdr = {
        let len = None;
        let nl_type = Rtm::Getqdisc;
        let flags = vec![NlmF::Request, NlmF::Dump];
        let seq = None;
        let pid = None;
        let payload = tcm;
        Nlmsghdr::new(len, nl_type, flags, seq, pid, payload)
    };

    socket.send_nl(nlhdr).unwrap();

    while let Ok(nlmsg) = socket.recv_nl::<u16, Tcmsg>(None) {
        let tcm = nlmsg.nl_payload;
        let handle = tcm.tcm_handle;
        let parent = tcm.tcm_parent;

        let ifname = match ifnames.get(&tcm.tcm_ifindex) {
            None => continue,
            Some(name) => name,
        };
        let mut push_counter = |kind: &String, name: &str, value: u64| {
            ret.push(QdiscStat {
                ifname: ifname.clone(),
                kind: (*kind).clone(),
                handle: handle,
                parent: parent,
                name: name.to_string(),
                value: value,
            });
        };

        let mut kind: String = "".to_string();
        for attr in tcm.rtattrs {
            match attr.rta_type {
                Tca::Kind => {
                    kind = std::str::from_utf8(&attr.rta_payload).unwrap().to_string();
                    // The string is NUL-terminated, so pop the last char.
                    kind.pop();
                }
                Tca::Stats2 => {
                    let mut buf = StreamReadBuffer::new(&attr.rta_payload);
                    buf.set_size_hint(attr.payload_size());
                    for nattr in Rtattrs::<TcaStats2, Vec<u8>>::deserialize(&mut buf).unwrap() {
                        let mut buf = StreamReadBuffer::new(&nattr.rta_payload);
                        match nattr.rta_type {
                            TcaStats2::Basic => {
                                let gnet_stats = GnetStatsBasic::deserialize(&mut buf).unwrap();
                                push_counter(&kind, "bytes", gnet_stats.bytes);
                                push_counter(&kind, "packets", gnet_stats.packets as u64);
                            }
                            TcaStats2::Queue => {
                                let gnet_stats = GnetStatsQueue::deserialize(&mut buf).unwrap();
                                push_counter(&kind, "qlen", gnet_stats.qlen as u64);
                                push_counter(&kind, "backlog", gnet_stats.backlog as u64);
                                push_counter(&kind, "drops", gnet_stats.drops as u64);
                                push_counter(&kind, "requeues", gnet_stats.requeues as u64);
                                push_counter(&kind, "overlimits", gnet_stats.overlimits as u64);
                            }
                            TcaStats2::RateEst => {
                                let gnet_stats =
                                    GnetStatsRateEst::<u32>::deserialize(&mut buf).unwrap();
                                push_counter(&kind, "bps", gnet_stats.bps as u64);
                                push_counter(&kind, "pps", gnet_stats.pps as u64);
                            }
                            TcaStats2::RateEst64 => {
                                let gnet_stats =
                                    GnetStatsRateEst::<u64>::deserialize(&mut buf).unwrap();
                                push_counter(&kind, "bps", gnet_stats.bps);
                                push_counter(&kind, "pps", gnet_stats.pps);
                            }
                            TcaStats2::App => {
                                // xxx Parse according to kind
                            }
                            _ => {}
                        }
                    }
                }
                Tca::Stats | Tca::Xstats => {
                    // xxx I think Stats is backward-compat combination of TcaStats2::Basic
                    // xxx and TcaStats2::Queue and Xstats backward-compat TcaStats2::App.
                }
                _ => {}
            }
        }
    }

    return ret;
}
