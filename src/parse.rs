use crate::ct;

use std::iter::Peekable;

trait Parser {
    fn parse(
        &self,
        words: &mut Peekable<std::slice::Iter<String>>,
    ) -> Result<Vec<ct::CounterRule>, String>;
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

fn peek_ifmatch(words: &mut Peekable<std::slice::Iter<String>>) -> Option<String> {
    if let Some(word) = words.peek() {
        if is_ifmatch(word) {
            return Some((*word).chars().skip(1).collect());
        }
    }
    None
}

fn parse_ifmatch(words: &mut Peekable<std::slice::Iter<String>>) -> Option<String> {
    if let Some(ifmatch) = peek_ifmatch(words) {
        words.next();
        Some(ifmatch)
    } else {
        None
    }
}

fn is_unit(word: &String) -> bool {
    if let Some(first) = word.chars().nth(0) {
        first == '/'
    } else {
        false
    }
}

struct EthtoolParser {}

// xxx temporary -- to be used for the CounterRule trait when it's written
pub struct EthtoolCounterRule {
    pub pat: glob::Pattern,
    pub unit: Option<ct::UnitChain>,
}

impl Parser for EthtoolParser {
    // Syntax: @ifmatch* [@if2match* ...] ctmatch* [ctmatch* ...]
    fn parse(
        &self,
        words: &mut Peekable<std::slice::Iter<String>>,
    ) -> Result<Vec<ct::CounterRule>, String> {
        if words.peek().is_none() {
            return Ok(Vec::new());
        }

        let mut ifmatches = Vec::new();
        while let Some(ifmatch) = parse_ifmatch(words) {
            match glob::Pattern::new(&ifmatch) {
                Ok(pat) => ifmatches.push(pat),
                Err(err) => return Err(err.msg.to_string()),
            }
        }
        if ifmatches.is_empty() {
            return Err("Expected one or more @ifmatches".to_string());
        }

        let mut ctmatches = Vec::<String>::new();
        while let Some(word) = words.peek() {
            if is_ns(word) || is_ifmatch(word) {
                break;
            }
            ctmatches.push((*word).clone());
            words.next();
        }

        if ctmatches.is_empty() {
            ctmatches.push("*".to_string());
        }

        let mut ret = Vec::new();
        for ifmatch in &ifmatches {
            for ctmatch in &ctmatches {
                ret.push(ct::CounterRule {
                    pat: (*ifmatch).clone(),
                    unit: None,
                });
            }
        }

        Ok(ret)
    }
}

const PARSERS: [(&str, &dyn Parser); 1] = [("ethtool", &EthtoolParser {})];

pub fn parse_expr(
    words: &mut Peekable<std::slice::Iter<String>>,
) -> Result<Vec<ct::CounterRule>, String> {
    let ns = parse_ns_opt(words).unwrap_or("ethtool".to_string());
    PARSERS
        .iter()
        .find(|(name, _)| *name == ns)
        .ok_or(format!("Unknown namespace: {}", ns))?
        .1
        .parse(words)
}
