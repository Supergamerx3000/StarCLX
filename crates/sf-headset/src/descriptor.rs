//! Liest aus dem HID-Report-Descriptor die Felder, die zur Telefonie gehören:
//! wo im Report ein Feld liegt (Report-ID, Bit-Position, Breite) und welche
//! Usage es hat. Nur so viel HID, wie Headsets brauchen.

/// Usage Page in den oberen 16 Bit, Usage ID in den unteren
pub type Usage = u32;

pub const fn usage(page: u16, id: u16) -> Usage {
    (page as u32) << 16 | id as u32
}

const PAGE_TELEPHONY: u16 = 0x0B;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Values {
    /// Ein Wert je Usage (Taste gedrückt, LED an)
    Variable(Usage),
    /// Liste gerade gedrückter Tasten; Wert minus `min` ist der Index in `usages`
    Array { usages: Vec<Usage>, min: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub report_id: u8,
    /// Bit-Position nach der Report-ID
    pub offset: usize,
    pub size: usize,
    /// Relative Tasten melden nur den Druck, nicht den Zustand
    pub relative: bool,
    pub values: Values,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Telephony {
    pub inputs: Vec<Field>,
    pub outputs: Vec<Field>,
    /// Der Descriptor nutzt Report-IDs (erstes Byte jedes Reports)
    pub numbered: bool,
    /// Länge der Output-Reports in Bit, ohne Report-ID
    pub output_bits: Vec<(u8, usize)>,
}

impl Telephony {
    pub fn has_input(&self, u: Usage) -> bool {
        self.inputs.iter().any(|f| f.has(u))
    }

    pub fn has_output(&self, u: Usage) -> bool {
        self.outputs.iter().any(|f| f.has(u))
    }
}

impl Field {
    fn has(&self, u: Usage) -> bool {
        match &self.values {
            Values::Variable(v) => *v == u,
            Values::Array { usages, .. } => usages.contains(&u),
        }
    }
}

#[derive(Clone, Default)]
struct Globals {
    page: u16,
    logical_min: i32,
    report_size: usize,
    report_id: u8,
    report_count: usize,
}

/// Wertet den Descriptor aus. Felder ausserhalb einer Telefonie-Sammlung
/// (Tastatur, Lautstärke usw.) werden nur mitgezählt, damit die
/// Bit-Positionen stimmen.
pub fn parse(desc: &[u8]) -> Telephony {
    let mut out = Telephony::default();
    let mut g = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    let mut usages: Vec<Usage> = Vec::new();
    let mut usage_min: Option<Usage> = None;
    // Je Sammlung: gehört sie zur Telefonie?
    let mut collections: Vec<bool> = Vec::new();
    // Bit-Zähler je (Input/Output, Report-ID)
    let mut bits: Vec<(bool, u8, usize)> = Vec::new();

    let mut i = 0;
    while i < desc.len() {
        let prefix = desc[i];
        if prefix == 0xFE {
            // Long Item: Länge im nächsten Byte
            let len = desc.get(i + 1).copied().unwrap_or(0) as usize;
            i += 3 + len;
            continue;
        }
        let size = match prefix & 3 {
            3 => 4,
            n => n as usize,
        };
        let Some(data) = desc.get(i + 1..i + 1 + size) else {
            break;
        };
        i += 1 + size;
        let unsigned = data
            .iter()
            .rev()
            .fold(0u32, |acc, b| acc << 8 | u32::from(*b));
        let signed = match size {
            1 => i32::from(data[0] as i8),
            2 => i32::from(i16::from_le_bytes([data[0], data[1]])),
            4 => unsigned as i32,
            _ => 0,
        };
        let full_usage = |id: u32| {
            if size == 4 {
                id
            } else {
                usage(g.page, id as u16)
            }
        };
        match (prefix >> 2) & 3 {
            // Main
            0 => {
                match prefix >> 4 {
                    0x8 | 0x9 => {
                        let input = prefix >> 4 == 0x8;
                        let total = g.report_size * g.report_count;
                        let counter = match bits
                            .iter_mut()
                            .find(|(inp, id, _)| *inp == input && *id == g.report_id)
                        {
                            Some(c) => c,
                            None => {
                                bits.push((input, g.report_id, 0));
                                bits.last_mut().unwrap()
                            }
                        };
                        let start = counter.2;
                        counter.2 += total;
                        let constant = unsigned & 1 != 0;
                        let variable = unsigned & 2 != 0;
                        let relative = unsigned & 4 != 0;
                        if !constant && collections.contains(&true) && g.report_size > 0 {
                            let fields = if input {
                                &mut out.inputs
                            } else {
                                &mut out.outputs
                            };
                            if variable {
                                for n in 0..g.report_count {
                                    let u = usages.get(n).or(usages.last()).copied();
                                    if let Some(u) = u {
                                        fields.push(Field {
                                            report_id: g.report_id,
                                            offset: start + n * g.report_size,
                                            size: g.report_size,
                                            relative,
                                            values: Values::Variable(u),
                                        });
                                    }
                                }
                            } else if !usages.is_empty() {
                                for n in 0..g.report_count {
                                    fields.push(Field {
                                        report_id: g.report_id,
                                        offset: start + n * g.report_size,
                                        size: g.report_size,
                                        relative,
                                        values: Values::Array {
                                            usages: usages.clone(),
                                            min: g.logical_min,
                                        },
                                    });
                                }
                            }
                        }
                    }
                    0xA => {
                        // Collection: Telefonie, wenn sie selbst oder eine
                        // äussere zur Telefonie gehört
                        let own = usages
                            .first()
                            .is_some_and(|u| (u >> 16) as u16 == PAGE_TELEPHONY);
                        collections.push(own || collections.last().copied().unwrap_or(false));
                    }
                    0xC => {
                        collections.pop();
                    }
                    _ => {}
                }
                usages.clear();
                usage_min = None;
            }
            // Global
            1 => match prefix >> 4 {
                0x0 => g.page = unsigned as u16,
                0x1 => g.logical_min = signed,
                0x7 => g.report_size = unsigned as usize,
                0x8 => {
                    g.report_id = unsigned as u8;
                    out.numbered = true;
                }
                0x9 => g.report_count = unsigned as usize,
                0xA => stack.push(g.clone()),
                0xB => {
                    if let Some(s) = stack.pop() {
                        g = s;
                    }
                }
                _ => {}
            },
            // Local
            2 => match prefix >> 4 {
                0x0 => usages.push(full_usage(unsigned)),
                0x1 => usage_min = Some(full_usage(unsigned)),
                0x2 => {
                    if let Some(min) = usage_min.take() {
                        let max = full_usage(unsigned);
                        // Schutz vor kaputten Descriptoren
                        if max >= min && max - min < 1024 {
                            usages.extend(min..=max);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    out.output_bits = bits
        .into_iter()
        .filter(|(input, ..)| !input)
        .map(|(_, id, n)| (id, n))
        .collect();
    out
}

/// Liest `size` Bit ab Bit `offset` (little endian, wie HID)
pub fn get_bits(data: &[u8], offset: usize, size: usize) -> u32 {
    let mut v = 0u32;
    for n in 0..size.min(32) {
        let bit = offset + n;
        if data.get(bit / 8).is_some_and(|b| b >> (bit % 8) & 1 == 1) {
            v |= 1 << n;
        }
    }
    v
}

pub fn set_bits(data: &mut [u8], offset: usize, size: usize, value: u32) {
    for n in 0..size.min(32) {
        let bit = offset + n;
        if let Some(b) = data.get_mut(bit / 8) {
            if value >> n & 1 == 1 {
                *b |= 1 << (bit % 8);
            } else {
                *b &= !(1 << (bit % 8));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const HOOK: Usage = usage(0x0B, 0x20);
    pub const MUTE: Usage = usage(0x0B, 0x2F);
    pub const LED_OFF_HOOK: Usage = usage(0x08, 0x17);
    pub const LED_MUTE: Usage = usage(0x08, 0x09);
    pub const RINGER: Usage = usage(0x0B, 0x9E);

    /// Aufbau wie bei Jabra-Headsets: Lautstärke (Consumer) mit eigener
    /// Report-ID, dann Telefonie mit Tasten und LEDs.
    pub const JABRA_LIKE: &[u8] = &[
        0x05, 0x0C, // Usage Page (Consumer)
        0x09, 0x01, // Usage (Consumer Control)
        0xA1, 0x01, // Collection (Application)
        0x85, 0x01, //   Report ID 1
        0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x02, //
        0x09, 0xE9, 0x09, 0xEA, // Volume Up/Down
        0x81, 0x02, //   Input (Var)
        0x95, 0x06, 0x81, 0x01, //   Padding
        0xC0, // End Collection
        0x05, 0x0B, // Usage Page (Telephony)
        0x09, 0x05, // Usage (Headset)
        0xA1, 0x01, // Collection (Application)
        0x85, 0x02, //   Report ID 2
        0x15, 0x00, 0x25, 0x01, 0x75, 0x01, //
        0x09, 0x20, // Hook Switch
        0x95, 0x01, 0x81, 0x02, //   Input (Var, Abs)
        0x09, 0x2F, // Phone Mute
        0x81, 0x06, //   Input (Var, Rel)
        0x95, 0x06, 0x81, 0x01, //   Padding
        0x05, 0x08, // Usage Page (LED)
        0x09, 0x17, 0x09, 0x09, 0x09, 0x18, // Off-Hook, Mute, Ring
        0x95, 0x03, 0x91, 0x02, //   Output (Var)
        0x05, 0x0B, 0x09, 0x9E, // Ringer
        0x95, 0x01, 0x91, 0x02, //   Output (Var)
        0x95, 0x04, 0x91, 0x01, //   Padding
        0xC0, // End Collection
    ];

    #[test]
    fn finds_telephony_fields() {
        let t = parse(JABRA_LIKE);
        assert!(t.numbered);
        assert_eq!(
            t.inputs,
            vec![
                Field {
                    report_id: 2,
                    offset: 0,
                    size: 1,
                    relative: false,
                    values: Values::Variable(HOOK)
                },
                Field {
                    report_id: 2,
                    offset: 1,
                    size: 1,
                    relative: true,
                    values: Values::Variable(MUTE)
                },
            ]
        );
        assert!(t.has_output(LED_OFF_HOOK) && t.has_output(LED_MUTE) && t.has_output(RINGER));
        let ring = t.outputs.iter().find(|f| f.has(RINGER)).unwrap();
        assert_eq!((ring.report_id, ring.offset), (2, 3));
        assert_eq!(t.output_bits, vec![(2, 8)]);
    }

    #[test]
    fn ignores_devices_without_telephony() {
        let mouse = [
            0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00,
            0x25, 0x01, 0x75, 0x01, 0x95, 0x03, 0x81, 0x02, 0xC0,
        ];
        let t = parse(&mouse);
        assert!(t.inputs.is_empty() && t.outputs.is_empty());
        assert!(!t.numbered);
    }

    #[test]
    fn array_buttons() {
        let desc = [
            0x05, 0x0B, 0x09, 0x05, 0xA1, 0x01, // Telephony Headset
            0x15, 0x01, 0x25, 0x02, 0x75, 0x08, 0x95, 0x01, //
            0x09, 0x20, 0x09, 0x2F, // Hook, Mute
            0x81, 0x00, // Input (Array)
            0xC0,
        ];
        let t = parse(&desc);
        assert_eq!(
            t.inputs[0].values,
            Values::Array {
                usages: vec![HOOK, MUTE],
                min: 1
            }
        );
    }

    #[test]
    fn bit_access() {
        let mut d = [0u8; 2];
        set_bits(&mut d, 3, 1, 1);
        set_bits(&mut d, 6, 4, 0b1011);
        assert_eq!(d, [0b1100_1000, 0b10]);
        assert_eq!(get_bits(&d, 6, 4), 0b1011);
        set_bits(&mut d, 3, 1, 0);
        assert_eq!(get_bits(&d, 3, 1), 0);
    }
}
