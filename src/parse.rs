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
struct EthtoolCounterMatch {
    pat: glob::Pattern,
    unit: ct::UnitChain,
}
struct EthtoolCounterRule {
    ifmatches: Vec<glob::Pattern>,
    ctmatches: Vec<EthtoolCounterMatch>,
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
                        });
                        break;
                    }
                }
            }
        }
        Ok(ret)
    }
    fn fmt(&self) -> String {
        format!(
            "Ethtool ifmatches={:?} ctmatches={:?}",
            self.ifmatches, self.ctmatches
        )
    }
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

        let mut ctmatches = Vec::<(glob::Pattern, Option<ct::UnitChain>)>::new();
        while let Some(word) = words.peek() {
            if is_ns(word) || is_ifmatch(word) {
                break;
            }
            if is_unit(word) {
                return Err(format!("Unexpected unit before counter: {}", word));
            }

            match glob::Pattern::new(&word) {
                Ok(pat) => ctmatches.push((pat, None)),
                Err(err) => return Err(err.msg.to_string()),
            }
            words.next();

            if let Some(u) = parse_unit(words)? {
                for ctmatch in ctmatches.iter_mut().rev() {
                    if ctmatch.1.is_none() {
                        ctmatch.1 = Some(u.clone());
                    } else {
                        break;
                    }
                }
            }
        }

        if ctmatches.is_empty() {
            ctmatches.push((glob::Pattern::new("*").unwrap(), None));
        }

        for ctmatch in ctmatches.iter_mut() {
            if ctmatch.1.is_none() {
                ctmatch.1 = Some(ct::UnitChain {
                    units: vec![ct::Unit {
                        prefix: ct::UPfx::None,
                        base: ct::UBase::Units,
                    }],
                    freq: ct::UFreq::PerSecond,
                });
            }
        }

        let ret: Vec<Box<dyn ct::CounterRule>> = vec![Box::new(EthtoolCounterRule {
            ifmatches: ifmatches,
            ctmatches: ctmatches
                .drain(..)
                .map(|(pat, unit)| EthtoolCounterMatch {
                    pat: pat,
                    unit: unit.unwrap(),
                })
                .collect(),
        })];

        Ok(ret)
    }
}

const PARSERS: [(&str, &dyn Parser); 1] = [("ethtool", &EthtoolParser {})];

pub fn parse_expr(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Result<Vec<Box<dyn ct::CounterRule>>, String> {
    let ns = parse_ns_opt(words).unwrap_or("ethtool".to_string());
    PARSERS
        .iter()
        .find(|(name, _)| *name == ns)
        .ok_or(format!("Unknown namespace: {}", ns))?
        .1
        .parse(words)
}
