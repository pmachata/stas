use crate::ct;
use crate::ethtool_ss;
use crate::netlink;

use std::iter::Peekable;

trait Parser {
    fn parse(
        &self,
        words: &mut Peekable<std::slice::Iter<String>>,
    ) -> Result<Vec<Box<dyn ct::CounterRule>>, String>;
}

fn is_ns(word: &String) -> bool {
    if let Some(last) = word.chars().last() {
        last == ':'
    } else {
        false
    }
}

fn peek_ns(words: &mut Peekable<std::slice::Iter<String>>) -> Option<String> {
    if let Some(word) = words.peek() {
        if is_ns(word) {
            let mut ret: String = (*word).clone();
            ret.pop();
            return Some(ret);
        }
    }
    None
}

fn parse_ns_opt(words: &mut Peekable<std::slice::Iter<String>>) -> Option<String> {
    if let Some(ns) = peek_ns(words) {
        words.next();
        Some(ns)
    } else {
        None
    }
}

fn is_ifmatch(word: &String) -> bool {
    if let Some(first) = word.chars().nth(0) {
        first == '@'
    } else {
        false
    }
}

fn peek_ifmatch(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Result<Option<glob::Pattern>, glob::PatternError> {
    if let Some(word) = words.peek() {
        if is_ifmatch(word) {
            return Some(glob::Pattern::new(
                &(*word).chars().skip(1).collect::<String>(),
            ))
            .transpose();
        }
    }
    Ok(None)
}

fn parse_ifmatch(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Result<Option<glob::Pattern>, glob::PatternError> {
    if let Some(ifmatch) = peek_ifmatch(words)? {
        words.next();
        Ok(Some(ifmatch))
    } else {
        Ok(None)
    }
}

fn is_unit(word: &String) -> bool {
    if let Some(first) = word.chars().nth(0) {
        first == '/'
    } else {
        false
    }
}

fn parse_unit_pfx<I>(it: &mut Peekable<I>) -> Result<ct::Unit, String>
where
    I: Iterator<Item = char>,
{
    let prefix = match it.peek() {
        Some(&'G') => {
            it.next();
            ct::UPfx::Giga
        }
        Some(&'M') => {
            it.next();
            ct::UPfx::Mega
        }
        Some(&'k') | Some(&'K') => {
            it.next();
            ct::UPfx::Kilo
        }
        Some(&'m') => {
            it.next();
            ct::UPfx::Milli
        }
        Some(&'u') => {
            it.next();
            ct::UPfx::Micro
        }
        Some(&'n') => {
            it.next();
            ct::UPfx::Nano
        }
        _ => ct::UPfx::None,
    };

    let base = match it.next() {
        Some('p') => ct::UBase::Packets,
        Some('s') => ct::UBase::Seconds,
        Some('B') => ct::UBase::Bytes,
        Some('b') => ct::UBase::Bits,
        Some('1') => ct::UBase::Units,
        Some(c) => {
            return Err(format!("Unknown unit, '{}'", c));
        }
        _ => {
            return Err("Missing unit".to_string());
        }
    };

    Ok(ct::Unit {
        prefix: prefix,
        base: base,
    })
}

fn parse_unit_freq(str: &str) -> Result<(ct::Unit, ct::UFreq), String> {
    let mut freq: Option<ct::UFreq> = None;
    let mut it = str.chars().peekable();

    if it.peek() == Some(&'d') {
        it.next();
        freq = Some(ct::UFreq::Delta);
    }

    let pfx = parse_unit_pfx(&mut it)?;

    let rest = it.collect::<String>();
    if rest.is_empty() {
        return Ok((pfx, freq.unwrap_or(ct::UFreq::AsIs)));
    }

    if freq.is_some() {
        return Err(format!("Unit suffix not expected: {}", rest));
    }

    if rest == "ps" {
        return Ok((pfx, ct::UFreq::PerSecond));
    }

    return Err(format!("Unit suffix not understood: {}", rest));
}

fn parse_unit_chain(str: &str) -> Result<ct::UnitChain, String> {
    let mut units = Vec::<ct::Unit>::new();
    let mut freq = ct::UFreq::AsIs;

    // The unit string starts with a '/', so skip the first (empty) element.
    for substr in str.split('/').skip(1) {
        let (unit, this_freq) = parse_unit_freq(substr)?;
        if this_freq != ct::UFreq::AsIs {
            if freq != ct::UFreq::AsIs {
                return Err("Only one frequency allowed in a unit chain.".to_string());
            }
            freq = this_freq;
        }
        units.push(unit);
    }

    Ok(ct::UnitChain {
        units: units,
        freq: freq,
    })
}

fn peek_unit(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Result<Option<ct::UnitChain>, String> {
    if let Some(word) = words.peek() {
        if is_unit(word) {
            return Ok(Some(parse_unit_chain(word)?));
        }
    }
    Ok(None)
}

fn parse_unit(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Result<Option<ct::UnitChain>, String> {
    if let Some(unit) = peek_unit(words)? {
        words.next();
        Ok(Some(unit))
    } else {
        Ok(None)
    }
}

#[derive(Debug)]
enum QdiscHandlePartMatch {
    Any,        // Wildcard, currently only used for minor
    None,       // Omitted, only makes sense for minor
    Value(u16), // A particular value
}
#[derive(Debug)]
struct QdiscHandleMatch {
    major: QdiscHandlePartMatch,
    minor: QdiscHandlePartMatch,
}

fn parse_hnmatch_one(word: &String) -> Option<QdiscHandleMatch> {
    let mut iter = word.chars();
    let major: String;
    if let Some(pos) = word.chars().position(|ch| !ch.is_digit(16)) {
        if pos == 0 {
            return None;
        }
        major = iter.by_ref().take(pos).collect();
    } else {
        return None;
    }

    match iter.next() {
        None => return None,
        Some(ch) => {
            if ch != ':' {
                return None;
            }
        }
    }
    let minor: String = iter.collect();
    if minor.is_empty() {
        return Some(QdiscHandleMatch {
            major: QdiscHandlePartMatch::Value(u16::from_str_radix(&major, 16).unwrap()),
            minor: QdiscHandlePartMatch::None,
        });
    } else if minor == "*" {
        return Some(QdiscHandleMatch {
            major: QdiscHandlePartMatch::Value(u16::from_str_radix(&major, 16).unwrap()),
            minor: QdiscHandlePartMatch::Any,
        });
    } else if minor.chars().all(|ch| ch.is_digit(16)) {
        return Some(QdiscHandleMatch {
            major: QdiscHandlePartMatch::Value(u16::from_str_radix(&major, 16).unwrap()),
            minor: QdiscHandlePartMatch::Value(u16::from_str_radix(&minor, 16).unwrap()),
        });
    } else {
        return None;
    }
}

fn peek_hnmatch(words: &mut Peekable<std::slice::Iter<String>>) -> Option<QdiscHandleMatch> {
    if let Some(word) = words.peek() {
        parse_hnmatch_one(word)
    } else {
        None
    }
}

fn parse_hnmatch(words: &mut Peekable<std::slice::Iter<String>>) -> Option<QdiscHandleMatch> {
    if let Some(hnmatch) = peek_hnmatch(words) {
        words.next();
        Some(hnmatch)
    } else {
        None
    }
}

fn parse_value_filter_one(word: &String) -> Option<Box<dyn ct::CounterValueFilter>> {
    if word == "non0" {
        Some(Box::new(ct::NonZeroCounterFilter {}))
    } else {
        None
    }
}

fn peek_value_filter(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Option<Box<dyn ct::CounterValueFilter>> {
    if let Some(word) = words.peek() {
        parse_value_filter_one(word)
    } else {
        None
    }
}

fn parse_value_filter(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Option<Box<dyn ct::CounterValueFilter>> {
    if let Some(vfilt) = peek_value_filter(words) {
        words.next();
        Some(vfilt)
    } else {
        None
    }
}

#[derive(Debug)]
struct CounterNameMatch {
    pat: glob::Pattern,
    unit: ct::UnitChain,
    vfilt: Vec<Box<dyn ct::CounterValueFilter>>,
}

#[derive(Debug)]
struct EthtoolCounterRule {
    ifmatches: Vec<glob::Pattern>,
    ctmatches: Vec<CounterNameMatch>,
}

impl ct::CounterRule for EthtoolCounterRule {
    fn counters(&self) -> Result<Vec<ct::CounterImm>, String> {
        let mut ret = Vec::new();
        for ifname in netlink::ifnames()
            .iter()
            .filter(|ifname| self.ifmatches.iter().any(|ref pat| pat.matches(&ifname)))
        {
            for stat in ethtool_ss::stats_for(&ifname) {
                for ctmatch in &self.ctmatches {
                    if ctmatch.pat.matches(&stat.name) {
                        ret.push(ct::CounterImm {
                            key: ct::CounterKey {
                                ctns: "ethtool",
                                ifname: ifname.clone(),
                                ctname: stat.name.clone(),
                            },
                            value: stat.value,
                            unit: ctmatch.unit.clone(),
                            filter: ctmatch.vfilt.iter().map(|vf| vf.clone_box()).collect(),
                        });
                        break;
                    }
                }
            }
        }
        Ok(ret)
    }
}

fn parse_ifmatches(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Result<Vec<glob::Pattern>, String> {
    let mut ifmatches = Vec::new();
    while let Some(pat) = match parse_ifmatch(words) {
        Ok(maybe_pat) => maybe_pat,
        Err(err) => return Err(err.msg.to_string()),
    } {
        ifmatches.push(pat);
    }
    if ifmatches.is_empty() {
        return Err("Expected one or more @ifmatches".to_string());
    }

    Ok(ifmatches)
}

fn parse_hnmatches(words: &mut Peekable<std::slice::Iter<String>>) -> Vec<QdiscHandleMatch> {
    let mut hnmatches = Vec::new();
    while let Some(pat) = parse_hnmatch(words) {
        hnmatches.push(pat);
    }
    if hnmatches.is_empty() {
        hnmatches.push(QdiscHandleMatch {
            major: QdiscHandlePartMatch::Any,
            minor: QdiscHandlePartMatch::Any,
        });
    }
    hnmatches
}

fn parse_ctmatches(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Result<Vec<CounterNameMatch>, String> {
    struct Ctmatch {
        pat: glob::Pattern,
        unit: Option<ct::UnitChain>,
        vfilt: Vec<Box<dyn ct::CounterValueFilter>>,
    }
    let mut ctmatches = Vec::<Ctmatch>::new();
    while let Some(word) = words.peek() {
        if is_ns(word) || is_ifmatch(word) {
            break;
        }
        if is_unit(word) {
            return Err(format!("Unexpected unit before counter: {}", word));
        }

        let mut ctmatch;
        match glob::Pattern::new(&word) {
            Ok(pat) => {
                ctmatch = Ctmatch {
                    pat: pat,
                    unit: None,
                    vfilt: Vec::new(),
                };
            }
            Err(err) => return Err(err.msg.to_string()),
        }
        words.next();

        while let Some(vf) = parse_value_filter(words) {
            ctmatch.vfilt.push(vf);
        }

        ctmatches.push(ctmatch);

        if let Some(u) = parse_unit(words)? {
            for ctmatch in ctmatches.iter_mut().rev() {
                if ctmatch.unit.is_none() {
                    ctmatch.unit = Some(u.clone());
                } else {
                    break;
                }
            }
        }
    }

    if ctmatches.is_empty() {
        ctmatches.push(Ctmatch {
            pat: glob::Pattern::new("*").unwrap(),
            unit: None,
            vfilt: Vec::new(),
        });
    }

    for ctmatch in ctmatches.iter_mut() {
        if ctmatch.unit.is_none() {
            ctmatch.unit = Some(ct::UnitChain {
                units: vec![ct::Unit {
                    prefix: ct::UPfx::None,
                    base: ct::UBase::Units,
                }],
                freq: ct::UFreq::PerSecond,
            });
        }
    }

    Ok(ctmatches
        .drain(..)
        .map(|ctmatch| CounterNameMatch {
            pat: ctmatch.pat,
            unit: ctmatch.unit.unwrap(),
            vfilt: ctmatch.vfilt,
        })
        .collect())
}

struct EthtoolParser {}

impl Parser for EthtoolParser {
    // Syntax: @ifmatch* [@if2match* ...] ctmatch* [ctmatch* ...]
    fn parse(
        &self,
        words: &mut Peekable<std::slice::Iter<String>>,
    ) -> Result<Vec<Box<dyn ct::CounterRule>>, String> {
        if words.peek().is_none() {
            return Ok(Vec::new());
        }

        let ret: Vec<Box<dyn ct::CounterRule>> = vec![Box::new(EthtoolCounterRule {
            ifmatches: parse_ifmatches(words)?,
            ctmatches: parse_ctmatches(words)?,
        })];

        Ok(ret)
    }
}

#[derive(Debug)]
struct QdiscCounterRule {
    ifmatches: Vec<glob::Pattern>,
    hnmatches: Vec<QdiscHandleMatch>,
    // xxx Qdisc counters have a known unit. Implement it as a fallback for unspecified units.
    ctmatches: Vec<CounterNameMatch>,
}

impl ct::CounterRule for QdiscCounterRule {
    fn counters(&self) -> Result<Vec<ct::CounterImm>, String> {
        let mut ret = Vec::new();
        for qdisc_stat in netlink::qdiscs() {
            if !self
                .ifmatches
                .iter()
                .any(|ref pat| pat.matches(&qdisc_stat.ifname))
            {
                continue;
            }

            let mut hnmatched = false;
            let hnmajor: u16 = (qdisc_stat.handle >> 16) as u16;
            let pnmajor: u16 = (qdisc_stat.parent >> 16) as u16;
            let pnminor: u16 = (qdisc_stat.parent & 0xffffu32) as u16;
            for hnmatch in &self.hnmatches {
                let match_major = match hnmatch.minor {
                    QdiscHandlePartMatch::None => {
                        // <major>:, the given qdisc
                        hnmajor
                    }
                    QdiscHandlePartMatch::Any => {
                        // <major>:*, all qdiscs under major parent
                        pnmajor
                    }
                    QdiscHandlePartMatch::Value(minor) => {
                        // <major>:<minor>, qdisc with the given parent
                        if minor != pnminor {
                            continue;
                        }
                        pnmajor
                    }
                };
                match hnmatch.major {
                    QdiscHandlePartMatch::Any => {}
                    QdiscHandlePartMatch::None => {}
                    QdiscHandlePartMatch::Value(major) => {
                        if major != match_major {
                            continue;
                        }
                    }
                }
                hnmatched = true;
                break;
            }
            if !hnmatched {
                continue;
            }

            for ctmatch in &self.ctmatches {
                if ctmatch.pat.matches(&qdisc_stat.name) {
                    let ctname = format!(
                        "{} {:x}:{:x} {:x}: {}",
                        qdisc_stat.kind, pnmajor, pnminor, hnmajor, qdisc_stat.name
                    );
                    ret.push(ct::CounterImm {
                        key: ct::CounterKey {
                            ctns: "qdisc",
                            ifname: qdisc_stat.ifname.clone(), // xxx clone?
                            ctname: ctname,
                        },
                        value: qdisc_stat.value,
                        unit: ctmatch.unit.clone(),
                        filter: ctmatch.vfilt.iter().map(|vf| vf.clone_box()).collect(),
                    });
                    break;
                }
            }
        }
        Ok(ret)
    }
}

struct QdiscParser {}

impl Parser for QdiscParser {
    fn parse(
        &self,
        words: &mut Peekable<std::slice::Iter<String>>,
    ) -> Result<Vec<Box<dyn ct::CounterRule>>, String> {
        if words.peek().is_none() {
            return Ok(Vec::new());
        }

        let ifmatches = parse_ifmatches(words)?;
        let hnmatches = parse_hnmatches(words);
        let ctmatches = parse_ctmatches(words)?;

        let ret: Vec<Box<dyn ct::CounterRule>> = vec![Box::new(QdiscCounterRule {
            ifmatches: ifmatches,
            hnmatches: hnmatches,
            ctmatches: ctmatches,
        })];
        Ok(ret)
    }
}

const PARSERS: [(&str, &dyn Parser); 2] =
    [("ethtool", &EthtoolParser {}), ("qdisc", &QdiscParser {})];

pub fn parse_expr(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Result<Vec<Box<dyn ct::CounterRule>>, String> {
    let mut ret = Vec::new();
    loop {
        let ns = parse_ns_opt(words).unwrap_or("ethtool".to_string());
        let mut nv = PARSERS
            .iter()
            .find(|(name, _)| *name == ns)
            .ok_or(format!("Unknown namespace: {}", ns))?
            .1
            .parse(words)?;
        if nv.is_empty() {
            break;
        }
        ret.append(&mut nv);
    }
    Ok(ret)
}
