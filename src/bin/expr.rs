use std::env;
use std::iter::Peekable;

fn parse_ns_opt<I>(words: &mut Peekable<I>) -> Option<String>
where
    I: Iterator<Item = String>,
{
    if let Some(word) = words.peek() {
        if let Some(last) = word.chars().last() {
            if last == ':' {
                let mut ret = word.clone();
                ret.pop();
                words.next();
                return Some(ret);
            }
        }
    }
    None
}

fn parse_ifmatch_group<I>(words: &mut Peekable<I>, ret: &mut Vec<String>)
where
    I: Iterator<Item = String>,
{
    while let Some(word) = words.next() {
        if word == ")" {
            return;
        }
        ret.push(word.clone());
    }
}

fn parse_ifmatch<I>(words: &mut Peekable<I>) -> Vec<String>
where
    I: Iterator<Item = String>,
{
    let mut ret = Vec::<String>::new();
    if let Some(word) = words.next() {
        if word == "(" {
            parse_ifmatch_group(words, &mut ret);
        } else {
            ret.push(word.clone());
        }
    }
    ret
}

struct CounterMatch {}

fn parse_ctrmatch<I>(_words: &mut Peekable<I>)
where
    I: Iterator<Item = String>,
{
}

struct CounterExpr {
    ifmatch: Vec<String>,
}

fn parse_ctrex<I>(words: &mut Peekable<I>) -> CounterExpr
where
    I: Iterator<Item = String>,
{
    let ifmatch = parse_ifmatch(words);
    parse_ctrmatch(words);
    CounterExpr { ifmatch: ifmatch }
}

fn parse_expr_2<I>(words: &mut Peekable<I>)
where
    I: Iterator<Item = String>,
{
    let ns_opt = parse_ns_opt(words);
    let ctrex = parse_ctrex(words);

    match ns_opt {
        Some(ns) => print!("{}: ", ns),
        None => print!("<>"),
    }
    print!("( ");
    for ifmatch in ctrex.ifmatch {
        print!("{} ", ifmatch);
    }
    print!(") ");
    print!("\n");
}

fn parse_expr<I>(words: I)
where
    I: Iterator<Item = String>,
{
    parse_expr_2(&mut words.peekable())
}

fn main() {
    parse_expr(env::args().skip(1));
}
