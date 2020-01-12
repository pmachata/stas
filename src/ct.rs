extern crate fixed;
extern crate glob;

// Counters are generally 64-bit quantities. To support displaying deltas up to that resolution, we
// need an extra bit. And then to represent fractional values based off a 64-bit quantity, we need
// more bits for the fraction. To keep things simple, use a 128-bit fixpoint value split to 65 bits
// interal part and 63 bits fractional.
pub type Value = fixed::types::I65F63;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum UBase {
    Units,
    Packets,
    Seconds,
    Bits,
    Bytes,
}

static UNITS: [(UBase, char); 5] = [
    (UBase::Units, '1'),
    (UBase::Packets, 'p'),
    (UBase::Seconds, 's'),
    (UBase::Bits, 'b'),
    (UBase::Bytes, 'B'),
];

impl std::string::ToString for UBase {
    fn to_string(&self) -> String {
        UNITS
            .iter()
            .find(|&(unit, _)| unit == self)
            .unwrap()
            .1
            .to_string()
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum UPfx {
    Nano,
    Micro,
    Milli,
    None,
    Kilo,
    Mega,
    Giga,
    Tera,
    Peta,
    Exa,
}

static PREFIXES: [(UPfx, char); 10] = [
    (UPfx::Nano, 'n'),
    (UPfx::Micro, 'u'),
    (UPfx::Milli, 'm'),
    (UPfx::None, ' '),
    (UPfx::Kilo, 'K'),
    (UPfx::Mega, 'M'),
    (UPfx::Giga, 'G'),
    (UPfx::Tera, 'T'),
    (UPfx::Peta, 'P'),
    (UPfx::Exa, 'E'),
];

#[derive(Clone, Debug)]
pub struct Unit {
    pub prefix: UPfx,
    pub base: UBase,
}

#[derive(PartialEq, Clone, Debug)]
pub enum UFreq {
    AsIs,
    Delta,
    PerSecond,
}

#[derive(Clone, Debug)]
pub struct UnitChain {
    pub units: Vec<Unit>,
    pub freq: UFreq,
}

pub fn humanize(
    mut value: Value,
    base: UPfx,
    unit_prefix_str: &str,
    unit_str: &String,
    always_decimal: bool,
) -> String {
    let mut pos = PREFIXES.iter().position(|(unit, _)| *unit == base).unwrap();
    let mut trivial = !always_decimal;

    while value.abs() >= 1100 && (pos + 1) < PREFIXES.len() {
        value /= 1000;
        pos += 1;
        trivial = false;
    }

    if trivial {
        format!(
            "{}{:.0}    {}{}",
            unit_prefix_str, value, PREFIXES[pos].1, &unit_str
        )
    } else {
        format!(
            "{}{:.2} {}{}",
            unit_prefix_str, value, PREFIXES[pos].1, &unit_str
        )
    }
}

#[derive(Eq, PartialEq)]
pub struct CounterKey {
    pub ctns: &'static str,
    pub ifname: String,
    pub ctname: String,
}

pub struct CounterImm {
    pub key: CounterKey,
    pub value: u64,
    pub unit: UnitChain,
}

pub trait CounterRule {
    fn counters(&self) -> Result<Vec<CounterImm>, String>;
    fn fmt(&self) -> String;
}

pub fn convert(
    uchain: &UnitChain,
    value: Option<Value>,
    avg: Option<Value>,
) -> (Option<Value>, Option<Value>, Unit) {
    assert!(!uchain.units.is_empty());

    let mut it = uchain.units.iter();
    let mut prev_unit = it.next().unwrap();
    let ret_prefix = prev_unit.prefix;
    let mut ret_value = value;
    let mut ret_avg = avg;

    for unit in it {
        match (prev_unit.base, unit.base) {
            (UBase::Bytes, UBase::Bits) => {
                ret_value = ret_value.map(|v| v * 8);
                ret_avg = ret_avg.map(|av| av * 8);
            }
            (UBase::Bits, UBase::Bytes) => {
                ret_value = ret_value.map(|v| v / 8);
                ret_avg = avg.map(|av| av / 8);
            }
            _ => {}
        }
        prev_unit = unit;
    }

    (
        ret_value,
        ret_avg,
        Unit {
            prefix: ret_prefix,
            base: prev_unit.base,
        },
    )
}
