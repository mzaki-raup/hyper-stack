//! The permission bits of a remote file, as a number and as nine boxes.
//!
//! The two ways of saying it have to agree at all times: typing `640` must
//! tick the boxes, and ticking a box must change the number. Both live here
//! so the dialog is only layout, and so the arithmetic can be tested
//! without a server.

/// Who a permission applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Owner,
    Group,
    Other,
}

/// What is permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit {
    Read,
    Write,
    Execute,
}

impl Who {
    /// How far the three bits are shifted for this audience.
    fn shift(self) -> u32 {
        match self {
            Who::Owner => 6,
            Who::Group => 3,
            Who::Other => 0,
        }
    }

    pub const ALL: [Who; 3] = [Who::Owner, Who::Group, Who::Other];
}

impl Bit {
    fn value(self) -> u32 {
        match self {
            Bit::Read => 4,
            Bit::Write => 2,
            Bit::Execute => 1,
        }
    }

    pub const ALL: [Bit; 3] = [Bit::Read, Bit::Write, Bit::Execute];
}

/// Whether `mode` permits `bit` for `who`.
pub fn has(mode: u32, who: Who, bit: Bit) -> bool {
    mode & (bit.value() << who.shift()) != 0
}

/// `mode` with that permission set or cleared.
pub fn with(mode: u32, who: Who, bit: Bit, on: bool) -> u32 {
    let m = bit.value() << who.shift();
    if on { mode | m } else { mode & !m }
}

/// The nine permission bits of `mode`, which is all this dialog edits:
/// setuid, setgid and the sticky bit are left exactly as they were.
pub fn permission_bits(mode: u32) -> u32 {
    mode & 0o777
}

/// `mode` written the way `chmod` wants it, always three digits.
pub fn to_octal(mode: u32) -> String {
    format!("{:03o}", permission_bits(mode))
}

/// Read what the user typed. `None` when it is not a permission number, so
/// the dialog can leave the boxes alone rather than guess.
pub fn parse_octal(text: &str) -> Option<u32> {
    let t = text.trim();
    if t.is_empty() || t.len() > 4 || !t.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return None;
    }
    u32::from_str_radix(t, 8).ok().map(permission_bits)
}

/// `rwxr-x---`, the form `ls -l` shows, for the dialog to echo back.
pub fn to_symbolic(mode: u32) -> String {
    let mut out = String::with_capacity(9);
    for who in Who::ALL {
        for (bit, ch) in [(Bit::Read, 'r'), (Bit::Write, 'w'), (Bit::Execute, 'x')] {
            out.push(if has(mode, who, bit) { ch } else { '-' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_and_the_boxes_say_the_same_thing() {
        let mode = 0o640;
        assert!(has(mode, Who::Owner, Bit::Read));
        assert!(has(mode, Who::Owner, Bit::Write));
        assert!(!has(mode, Who::Owner, Bit::Execute));
        assert!(has(mode, Who::Group, Bit::Read));
        assert!(!has(mode, Who::Group, Bit::Write));
        assert!(!has(mode, Who::Other, Bit::Read));
        assert_eq!(to_symbolic(mode), "rw-r-----");
        assert_eq!(to_octal(mode), "640");
    }

    #[test]
    fn ticking_a_box_changes_only_that_bit() {
        let mode = 0o644;
        assert_eq!(to_octal(with(mode, Who::Other, Bit::Write, true)), "646");
        assert_eq!(to_octal(with(mode, Who::Owner, Bit::Execute, true)), "744");
        assert_eq!(to_octal(with(mode, Who::Owner, Bit::Read, false)), "244");
        // Setting what is already set changes nothing.
        assert_eq!(to_octal(with(mode, Who::Owner, Bit::Read, true)), "644");
    }

    #[test]
    fn every_box_survives_a_round_trip() {
        for mode in 0..0o1000u32 {
            let mut rebuilt = 0;
            for who in Who::ALL {
                for bit in Bit::ALL {
                    rebuilt = with(rebuilt, who, bit, has(mode, who, bit));
                }
            }
            assert_eq!(rebuilt, mode, "{mode:o} did not survive");
            assert_eq!(parse_octal(&to_octal(mode)), Some(mode));
        }
    }

    #[test]
    fn only_a_permission_number_is_accepted() {
        assert_eq!(parse_octal("755"), Some(0o755));
        assert_eq!(parse_octal(" 640 "), Some(0o640));
        assert_eq!(parse_octal("0755"), Some(0o755));
        assert_eq!(parse_octal("7"), Some(0o7));
        assert_eq!(parse_octal(""), None);
        assert_eq!(parse_octal("8"), None, "8 is not an octal digit");
        assert_eq!(parse_octal("rwx"), None);
        assert_eq!(parse_octal("77777"), None, "too long to be a mode");
    }

    #[test]
    fn the_special_bits_are_left_alone_by_the_editor() {
        // setuid + 755: the editor shows and writes back only the low nine.
        let mode = 0o4755;
        assert_eq!(to_octal(mode), "755");
        assert_eq!(permission_bits(mode), 0o755);
    }
}
