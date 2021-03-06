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

pub fn unit_units_ps() -> UnitChain {
    UnitChain {
        units: vec![Unit {
            prefix: UPfx::None,
            base: UBase::Units,
        }],
        freq: UFreq::PerSecond,
    }
}

pub fn unit_bytes() -> UnitChain {
    UnitChain {
        units: vec![Unit {
            prefix: UPfx::None,
            base: UBase::Bytes,
        }]
        .to_vec(),
        freq: UFreq::AsIs,
    }
}

pub fn unit_bytes_bits_ps() -> UnitChain {
    UnitChain {
        units: vec![
            Unit {
                prefix: UPfx::None,
                base: UBase::Bytes,
            },
            Unit {
                prefix: UPfx::None,
                base: UBase::Bits,
            },
        ]
        .to_vec(),
        freq: UFreq::PerSecond,
    }
}

pub fn unit_packets_ps() -> UnitChain {
    UnitChain {
        units: vec![Unit {
            prefix: UPfx::None,
            base: UBase::Packets,
        }]
        .to_vec(),
        freq: UFreq::PerSecond,
    }
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

pub trait CounterValueFilter: std::fmt::Debug {
    fn filter(&self, value: &Option<Value>, avg: &Option<Value>) -> bool;
    fn clone_box(&self) -> Box<dyn CounterValueFilter>;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub enum KeyHead {
    Ifname,
    Parent,
    Handle,
    Kind,
    Name,
}
pub const ALL_HEADS: [KeyHead; 5] = [
    KeyHead::Ifname,
    KeyHead::Parent,
    KeyHead::Handle,
    KeyHead::Kind,
    KeyHead::Name,
];

impl KeyHead {
    pub fn separate(self) -> bool {
        match self {
            KeyHead::Ifname | KeyHead::Parent | KeyHead::Name => true,
            KeyHead::Handle | KeyHead::Kind => false,
        }
    }
    pub fn suppress_dups(self) -> bool {
        match self {
            KeyHead::Ifname | KeyHead::Parent | KeyHead::Handle | KeyHead::Kind => true,
            KeyHead::Name => false,
        }
    }
    pub fn column_head(self) -> &'static str {
        match self {
            KeyHead::Ifname => "if",
            KeyHead::Parent => "par",
            KeyHead::Handle => "hnd",
            KeyHead::Kind => "kind",
            KeyHead::Name => "counter",
        }
    }
}

#[derive(Eq, PartialEq)]
pub struct CounterKey {
    pub ctns: &'static str,
    pub key: Vec<(KeyHead, String)>,
}

pub struct CounterImm {
    pub key: CounterKey,
    pub value: u64,
    pub unit: UnitChain,
    pub filter: Vec<Box<dyn CounterValueFilter>>,
}

pub trait CounterRule: std::fmt::Debug {
    fn counters(&self) -> Result<Vec<CounterImm>, String>;
}

#[derive(Debug, Clone)]
pub struct NonZeroCounterFilter {}

impl NonZeroCounterFilter {
    pub fn do_filter(&self, value: &Option<Value>, avg: &Option<Value>) -> bool {
        value.unwrap_or(Value::from_num(0)) != 0 || avg.unwrap_or(Value::from_num(0)) != 0
    }
}

impl CounterValueFilter for NonZeroCounterFilter {
    fn filter(&self, value: &Option<Value>, avg: &Option<Value>) -> bool {
        self.do_filter(&value, &avg)
    }
    fn clone_box(&self) -> Box<dyn CounterValueFilter> {
        Box::new(self.clone())
    }
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
