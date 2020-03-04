extern crate libc;
extern crate neli;

use crate::ct;
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

struct RtnlLinkStats<T> {
    rx_packets: T,
    tx_packets: T,
    rx_bytes: T,
    tx_bytes: T,
    rx_errors: T,
    tx_errors: T,
    rx_dropped: T,
    tx_dropped: T,
    multicast: T,
    collisions: T,
    rx_length_errors: T,
    rx_over_errors: T,
    rx_crc_errors: T,
    rx_frame_errors: T,
    rx_fifo_errors: T,
    rx_missed_errors: T,
    tx_aborted_errors: T,
    tx_carrier_errors: T,
    tx_fifo_errors: T,
    tx_heartbeat_errors: T,
    tx_window_errors: T,
    rx_compressed: T,
    tx_compressed: T,
    rx_nohandler: T,
}

impl<T> Nl for RtnlLinkStats<T>
where
    T: Nl,
{
    fn serialize(&self, mem: &mut StreamWriteBuffer) -> Result<(), SerError> {
        self.rx_packets.serialize(mem)?;
        self.tx_packets.serialize(mem)?;
        self.rx_bytes.serialize(mem)?;
        self.tx_bytes.serialize(mem)?;
        self.rx_errors.serialize(mem)?;
        self.tx_errors.serialize(mem)?;
        self.rx_dropped.serialize(mem)?;
        self.tx_dropped.serialize(mem)?;
        self.multicast.serialize(mem)?;
        self.collisions.serialize(mem)?;
        self.rx_length_errors.serialize(mem)?;
        self.rx_over_errors.serialize(mem)?;
        self.rx_crc_errors.serialize(mem)?;
        self.rx_frame_errors.serialize(mem)?;
        self.rx_fifo_errors.serialize(mem)?;
        self.rx_missed_errors.serialize(mem)?;
        self.tx_aborted_errors.serialize(mem)?;
        self.tx_carrier_errors.serialize(mem)?;
        self.tx_fifo_errors.serialize(mem)?;
        self.tx_heartbeat_errors.serialize(mem)?;
        self.tx_window_errors.serialize(mem)?;
        self.rx_compressed.serialize(mem)?;
        self.tx_compressed.serialize(mem)?;
        self.rx_nohandler.serialize(mem)?;
        Ok(())
    }

    fn deserialize<B>(mem: &mut StreamReadBuffer<B>) -> Result<Self, DeError>
    where
        B: AsRef<[u8]>,
    {
        Ok(RtnlLinkStats::<T> {
            rx_packets: T::deserialize(mem)?,
            tx_packets: T::deserialize(mem)?,
            rx_bytes: T::deserialize(mem)?,
            tx_bytes: T::deserialize(mem)?,
            rx_errors: T::deserialize(mem)?,
            tx_errors: T::deserialize(mem)?,
            rx_dropped: T::deserialize(mem)?,
            tx_dropped: T::deserialize(mem)?,
            multicast: T::deserialize(mem)?,
            collisions: T::deserialize(mem)?,
            rx_length_errors: T::deserialize(mem)?,
            rx_over_errors: T::deserialize(mem)?,
            rx_crc_errors: T::deserialize(mem)?,
            rx_frame_errors: T::deserialize(mem)?,
            rx_fifo_errors: T::deserialize(mem)?,
            rx_missed_errors: T::deserialize(mem)?,
            tx_aborted_errors: T::deserialize(mem)?,
            tx_carrier_errors: T::deserialize(mem)?,
            tx_fifo_errors: T::deserialize(mem)?,
            tx_heartbeat_errors: T::deserialize(mem)?,
            tx_window_errors: T::deserialize(mem)?,
            rx_compressed: T::deserialize(mem)?,
            tx_compressed: T::deserialize(mem)?,
            rx_nohandler: T::deserialize(mem)?,
        })
    }

    fn size(&self) -> usize {
        self.rx_packets.size()
            + self.tx_packets.size()
            + self.rx_bytes.size()
            + self.tx_bytes.size()
            + self.rx_errors.size()
            + self.tx_errors.size()
            + self.rx_dropped.size()
            + self.tx_dropped.size()
            + self.multicast.size()
            + self.collisions.size()
            + self.rx_length_errors.size()
            + self.rx_over_errors.size()
            + self.rx_crc_errors.size()
            + self.rx_frame_errors.size()
            + self.rx_fifo_errors.size()
            + self.rx_missed_errors.size()
            + self.tx_aborted_errors.size()
            + self.tx_carrier_errors.size()
            + self.tx_fifo_errors.size()
            + self.tx_heartbeat_errors.size()
            + self.tx_window_errors.size()
            + self.rx_compressed.size()
            + self.tx_compressed.size()
            + self.rx_nohandler.size()
    }
}

struct LinkInfo {
    index: i32,
    ifname: String,
    stats: Option<RtnlLinkStats<u64>>,
}

fn ifla_link_info(ifi: Nlmsghdr<u16, Ifinfomsg>) -> Option<LinkInfo> {
    let index = ifi.nl_payload.ifi_index;
    let mut ifname = None;
    let mut stats = None;

    for attr in ifi.nl_payload.rtattrs {
        let payload: Vec<u8> = attr.rta_payload;
        match attr.rta_type {
            Ifla::Ifname => {
                // Snip terminating zero.
                let prefix = &payload[..(payload.len() - 1)];
                ifname = Some(String::from_utf8_lossy(prefix).into_owned());
            }
            Ifla::Stats64 => {
                let mut buf = StreamReadBuffer::new(&payload);
                let stats64 = RtnlLinkStats::<u64>::deserialize(&mut buf).unwrap();
                stats = Some(stats64);
            }
            Ifla::Stats => {
                let mut buf = StreamReadBuffer::new(&payload);
                let stats32 = RtnlLinkStats::<u32>::deserialize(&mut buf).unwrap();
                stats = Some(RtnlLinkStats::<u64> {
                    rx_packets: stats32.rx_packets as u64,
                    tx_packets: stats32.tx_packets as u64,
                    rx_bytes: stats32.rx_bytes as u64,
                    tx_bytes: stats32.tx_bytes as u64,
                    rx_errors: stats32.rx_errors as u64,
                    tx_errors: stats32.tx_errors as u64,
                    rx_dropped: stats32.rx_dropped as u64,
                    tx_dropped: stats32.tx_dropped as u64,
                    multicast: stats32.multicast as u64,
                    collisions: stats32.collisions as u64,
                    rx_length_errors: stats32.rx_length_errors as u64,
                    rx_over_errors: stats32.rx_over_errors as u64,
                    rx_crc_errors: stats32.rx_crc_errors as u64,
                    rx_frame_errors: stats32.rx_frame_errors as u64,
                    rx_fifo_errors: stats32.rx_fifo_errors as u64,
                    rx_missed_errors: stats32.rx_missed_errors as u64,
                    tx_aborted_errors: stats32.tx_aborted_errors as u64,
                    tx_carrier_errors: stats32.tx_carrier_errors as u64,
                    tx_fifo_errors: stats32.tx_fifo_errors as u64,
                    tx_heartbeat_errors: stats32.tx_heartbeat_errors as u64,
                    tx_window_errors: stats32.tx_window_errors as u64,
                    rx_compressed: stats32.rx_compressed as u64,
                    tx_compressed: stats32.tx_compressed as u64,
                    rx_nohandler: stats32.rx_nohandler as u64,
                });
            }
            _ => {}
        }
    }

    if ifname.is_some() {
        Some(LinkInfo {
            index: index,
            ifname: ifname.unwrap(),
            stats: stats,
        })
    } else {
        None
    }
}

fn get_linkinfo() -> Vec<LinkInfo> {
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

    let mut linkinfo = Vec::new();
    while let Ok(nl) = socket.recv_nl::<u16, Ifinfomsg>(None) {
        if let Some(li) = ifla_link_info(nl) {
            linkinfo.push(li);
        }
    }
    linkinfo
}

pub fn ifnames() -> Vec<String> {
    get_linkinfo().drain(..).map(|li| li.ifname).collect()
}

pub fn ifindex_map() -> std::collections::HashMap<libc::c_int, String> {
    get_linkinfo()
        .drain(..)
        .map(|li| (li.index, li.ifname))
        .collect()
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
pub struct LinkStat {
    pub ifname: String,
    pub name: String,
    pub value: u64,
    pub default_unit: ct::UnitChain,
}

pub fn get_link_stats() -> Vec<LinkStat> {
    let mut link_stats = Vec::new();
    for li in get_linkinfo() {
        let ifname = li.ifname;
        if let Some(stats) = li.stats {
            let mut push = |name: &str, value: u64, default_unit: ct::UnitChain| {
                link_stats.push(LinkStat {
                    ifname: ifname.clone(),
                    name: name.to_string(),
                    value: value,
                    default_unit: default_unit,
                });
            };
            let pps = || ct::unit_packets_ps();
            push("rx_packets", stats.rx_packets, pps());
            push("tx_packets", stats.tx_packets, pps());
            push("rx_bytes", stats.rx_bytes, ct::unit_bytes_bits_ps());
            push("tx_bytes", stats.tx_bytes, ct::unit_bytes_bits_ps());
            push("rx_errors", stats.rx_errors, pps());
            push("tx_errors", stats.tx_errors, pps());
            push("rx_dropped", stats.rx_dropped, pps());
            push("tx_dropped", stats.tx_dropped, pps());
            push("multicast", stats.multicast, pps());
            push("collisions", stats.collisions, pps());
            push("rx_length_errors", stats.rx_length_errors, pps());
            push("rx_over_errors", stats.rx_over_errors, pps());
            push("rx_crc_errors", stats.rx_crc_errors, pps());
            push("rx_frame_errors", stats.rx_frame_errors, pps());
            push("rx_fifo_errors", stats.rx_fifo_errors, pps());
            push("rx_missed_errors", stats.rx_missed_errors, pps());
            push("tx_aborted_errors", stats.tx_aborted_errors, pps());
            push("tx_carrier_errors", stats.tx_carrier_errors, pps());
            push("tx_fifo_errors", stats.tx_fifo_errors, pps());
            push("tx_heartbeat_errors", stats.tx_heartbeat_errors, pps());
            push("tx_window_errors", stats.tx_window_errors, pps());
            push("rx_compressed", stats.rx_compressed, pps());
            push("tx_compressed", stats.tx_compressed, pps());
            push("rx_nohandler", stats.rx_nohandler, pps());
        }
    }
    link_stats
}

#[derive(Debug)]
pub struct QdiscStat {
    pub ifname: String,
    pub kind: String,
    pub handle: u32,
    pub parent: u32,
    pub name: String,
    pub value: u64,
    pub default_unit: ct::UnitChain,
}

struct QdiscStatsAux {
    stats: Vec<QdiscStat>,
    ifname: String,
    handle: u32,
    parent: u32,
}

impl QdiscStatsAux {
    fn new(ifname: &str, handle: u32, parent: u32) -> QdiscStatsAux {
        QdiscStatsAux {
            stats: Vec::new(),
            ifname: ifname.to_string(),
            handle: handle,
            parent: parent,
        }
    }
    fn push_counter(&mut self, kind: &String, name: &str, value: u64, default_unit: ct::UnitChain) {
        self.stats.push(QdiscStat {
            ifname: self.ifname.clone(),
            kind: (*kind).clone(),
            handle: self.handle,
            parent: self.parent,
            name: name.to_string(),
            value: value,
            default_unit: default_unit,
        });
    }
}

trait QdiscAppParser {
    fn parse_app(&self, kind: String, aux: &mut QdiscStatsAux, payload: &Vec<u8>);
}

#[derive(Debug)]
struct TcRedXstats {
    early: u32,  /* Early drops */
    pdrop: u32,  /* Drops due to queue limits */
    other: u32,  /* Drops due to drop() calls */
    marked: u32, /* Marked packets */
}

impl Nl for TcRedXstats {
    fn serialize(&self, mem: &mut StreamWriteBuffer) -> Result<(), SerError> {
        self.early.serialize(mem)?;
        self.pdrop.serialize(mem)?;
        self.other.serialize(mem)?;
        self.marked.serialize(mem)?;
        Ok(())
    }

    fn deserialize<B>(mem: &mut StreamReadBuffer<B>) -> Result<Self, DeError>
    where
        B: AsRef<[u8]>,
    {
        Ok(TcRedXstats {
            early: u32::deserialize(mem)?,
            pdrop: u32::deserialize(mem)?,
            other: u32::deserialize(mem)?,
            marked: u32::deserialize(mem)?,
        })
    }

    fn size(&self) -> usize {
        self.early.size() + self.pdrop.size() + self.other.size() + self.marked.size()
    }
}

struct QdiscAppParserRed {}

impl QdiscAppParser for QdiscAppParserRed {
    fn parse_app(&self, kind: String, aux: &mut QdiscStatsAux, payload: &Vec<u8>) {
        let mut buf = StreamReadBuffer::new(&payload);
        let xstats = TcRedXstats::deserialize(&mut buf).unwrap();
        aux.push_counter(&kind, "early", xstats.early as u64, ct::unit_packets_ps());
        aux.push_counter(&kind, "pdrop", xstats.pdrop as u64, ct::unit_packets_ps());
        aux.push_counter(&kind, "other", xstats.other as u64, ct::unit_packets_ps());
        aux.push_counter(&kind, "marked", xstats.marked as u64, ct::unit_packets_ps());
    }
}

const QDISC_APP_PARSERS: [(&str, &dyn QdiscAppParser); 1] = [("red", &QdiscAppParserRed {})];

pub fn qdiscs() -> Vec<QdiscStat> {
    let ifnames = ifindex_map();
    let mut ret = Vec::new();

    let mut socket = NlSocket::connect(NlFamily::Route, None, None, true).unwrap();
    let dump_invisible = Rtattr {
        rta_len: 4,
        rta_type: Tca::DumpInvisible,
        rta_payload: Vec::<u8>::new(),
    };
    let tcm = Tcmsg {
        tcm_family: 0,
        tcm_ifindex: 0,
        tcm_handle: 0,
        tcm_parent: 0,
        tcm_info: 0,
        rtattrs: Rtattrs::new(vec![dump_invisible]),
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
        let ifname = match ifnames.get(&tcm.tcm_ifindex) {
            None => continue,
            Some(name) => name,
        };
        let mut aux = QdiscStatsAux::new(ifname, tcm.tcm_handle, tcm.tcm_parent);

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
                                aux.push_counter(
                                    &kind,
                                    "bytes",
                                    gnet_stats.bytes,
                                    ct::unit_bytes_bits_ps(),
                                );
                                aux.push_counter(
                                    &kind,
                                    "packets",
                                    gnet_stats.packets as u64,
                                    ct::unit_packets_ps(),
                                );
                            }
                            TcaStats2::Queue => {
                                let gnet_stats = GnetStatsQueue::deserialize(&mut buf).unwrap();
                                aux.push_counter(
                                    &kind,
                                    "qlen",
                                    gnet_stats.qlen as u64,
                                    ct::unit_bytes(),
                                );
                                aux.push_counter(
                                    &kind,
                                    "backlog",
                                    gnet_stats.backlog as u64,
                                    ct::unit_bytes(),
                                );
                                aux.push_counter(
                                    &kind,
                                    "drops",
                                    gnet_stats.drops as u64,
                                    ct::unit_packets_ps(),
                                );
                                aux.push_counter(
                                    &kind,
                                    "requeues",
                                    gnet_stats.requeues as u64,
                                    ct::unit_packets_ps(),
                                );
                                aux.push_counter(
                                    &kind,
                                    "overlimits",
                                    gnet_stats.overlimits as u64,
                                    ct::unit_packets_ps(),
                                );
                            }
                            TcaStats2::RateEst => {
                                let gnet_stats =
                                    GnetStatsRateEst::<u32>::deserialize(&mut buf).unwrap();
                                aux.push_counter(
                                    &kind,
                                    "bps",
                                    gnet_stats.bps as u64,
                                    ct::unit_bytes_bits_ps(),
                                );
                                aux.push_counter(
                                    &kind,
                                    "pps",
                                    gnet_stats.pps as u64,
                                    ct::unit_packets_ps(),
                                );
                            }
                            TcaStats2::RateEst64 => {
                                let gnet_stats =
                                    GnetStatsRateEst::<u64>::deserialize(&mut buf).unwrap();
                                aux.push_counter(
                                    &kind,
                                    "bps",
                                    gnet_stats.bps,
                                    ct::unit_bytes_bits_ps(),
                                );
                                aux.push_counter(
                                    &kind,
                                    "pps",
                                    gnet_stats.pps,
                                    ct::unit_packets_ps(),
                                );
                            }
                            TcaStats2::App => {
                                if let Some((kind, parser)) =
                                    QDISC_APP_PARSERS.iter().find(|(a_kind, _)| *a_kind == kind)
                                {
                                    parser.parse_app(
                                        kind.to_string(),
                                        &mut aux,
                                        &nattr.rta_payload,
                                    );
                                }
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
        ret.extend(aux.stats.drain(..));
    }

    return ret;
}
