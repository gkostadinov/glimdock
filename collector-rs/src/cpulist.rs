//! Kernel CPU lists such as `0-15` or `20-23,28-31`, as printed by sysfs and
//! accepted by Proxmox's guest `affinity` option.
use anyhow::{bail, Result};
use std::collections::BTreeSet;

/// Highest logical CPU number accepted. Larger values are treated as malformed
/// so a hostile or broken list cannot expand into an unbounded array.
pub const MAX_CPU: u32 = 4095;

/// Parse a CPU list into sorted, unique CPU numbers.
///
/// Accepted: single numbers and ascending ranges separated by commas, with
/// optional blanks around each item (`0-3,8,10-11`). A list that is empty or
/// only blanks is an empty list. Anything else (empty items, signs, descending
/// or open ranges, stride syntax, numbers above [`MAX_CPU`]) is an error; a
/// malformed list is never partly accepted.
pub fn parse_cpu_list(input: &str) -> Result<Vec<u32>> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let mut cpus = BTreeSet::new();
    for item in input.split(',') {
        let item = item.trim();
        let (low, high) = match item.split_once('-') {
            Some((low, high)) => (number(low)?, number(high)?),
            None => {
                let single = number(item)?;
                (single, single)
            }
        };
        if low > high {
            bail!("CPU list range is descending");
        }
        cpus.extend(low..=high);
    }
    Ok(cpus.into_iter().collect())
}

fn number(text: &str) -> Result<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        bail!("CPU list item is not a number");
    }
    let value: u32 = text
        .parse()
        .map_err(|_| anyhow::anyhow!("CPU list number out of range"))?;
    if value > MAX_CPU {
        bail!("CPU list number out of range");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singles_ranges_and_mixed_lists() {
        assert_eq!(parse_cpu_list("3").unwrap(), [3]);
        assert_eq!(parse_cpu_list("0-3").unwrap(), [0, 1, 2, 3]);
        assert_eq!(parse_cpu_list("5-5").unwrap(), [5]);
        assert_eq!(
            parse_cpu_list("0-1,4,6-7").unwrap(),
            [0, 1, 4, 6, 7],
            "mixed ranges and singles"
        );
        assert_eq!(
            parse_cpu_list("20-23,28-31").unwrap(),
            [20, 21, 22, 23, 28, 29, 30, 31]
        );
    }

    #[test]
    fn output_is_sorted_and_unique_whatever_the_order() {
        assert_eq!(
            parse_cpu_list("8-9,0-1,1-2,9").unwrap(),
            [0, 1, 2, 8, 9],
            "overlaps and out-of-order items collapse"
        );
    }

    #[test]
    fn kernel_newline_and_blanks_are_ignored() {
        assert_eq!(parse_cpu_list("0-3\n").unwrap(), [0, 1, 2, 3]);
        assert_eq!(parse_cpu_list(" 0-1 , 4 ").unwrap(), [0, 1, 4]);
    }

    #[test]
    fn empty_list_is_valid_and_empty() {
        for empty in ["", "\n", "   \n"] {
            assert!(parse_cpu_list(empty).unwrap().is_empty(), "{empty:?}");
        }
    }

    #[test]
    fn malformed_lists_are_rejected_whole() {
        for bad in [
            "a",
            "1,,2",
            "1,",
            ",1",
            "1-",
            "-3",
            "-",
            "3-1",
            "1-2-3",
            "+1",
            "0x1",
            "1 2",
            "1;2",
            "0-3:2/4",
            "1.5",
            "४",
            "4096",
            "0-4096",
            "99999999999999999999",
        ] {
            assert!(parse_cpu_list(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn expansion_is_bounded_by_the_cpu_ceiling() {
        let all = parse_cpu_list("0-4095").unwrap();
        assert_eq!(all.len(), 4096);
        assert_eq!(all.last(), Some(&MAX_CPU));
    }
}
