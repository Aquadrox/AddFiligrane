use std::fmt;
use std::str::FromStr;

use crate::error::Error;

/// Pages auxquelles appliquer le filigrane (numérotation à partir de 1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PageSelection {
    #[default]
    All,
    Ranges(Vec<PageRange>),
}

/// Intervalle inclusif de pages ; `last == None` signifie « jusqu'à la fin ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRange {
    pub first: u32,
    pub last: Option<u32>,
}

impl PageRange {
    fn contains(&self, page: u32) -> bool {
        page >= self.first && self.last.is_none_or(|last| page <= last)
    }
}

impl PageSelection {
    /// Indique si la page `page` (à partir de 1) est sélectionnée.
    pub fn includes(&self, page: u32) -> bool {
        match self {
            Self::All => true,
            Self::Ranges(ranges) => ranges.iter().any(|range| range.contains(page)),
        }
    }

    /// Nombre de pages sélectionnées dans un document de `total` pages.
    pub fn count_in(&self, total: u32) -> u32 {
        match self {
            Self::All => total,
            Self::Ranges(_) => (1..=total).filter(|&page| self.includes(page)).count() as u32,
        }
    }

    /// Vérifie qu'au moins une page d'un document de `total` pages est sélectionnée.
    pub fn ensure_matches(&self, total: u32) -> Result<u32, Error> {
        match self.count_in(total) {
            0 => Err(Error::NoPageSelected {
                selection: self.to_string(),
                total,
            }),
            count => Ok(count),
        }
    }
}

impl FromStr for PageSelection {
    type Err = Error;

    /// Accepte par exemple `1-3, 7, 10-` ou `-5`. Une chaîne vide sélectionne tout.
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.is_empty() || input == "*" {
            return Ok(Self::All);
        }

        let mut ranges = Vec::new();
        for part in input.split([',', ';']) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            ranges.push(parse_range(part)?);
        }
        if ranges.is_empty() {
            return Ok(Self::All);
        }
        Ok(Self::Ranges(ranges))
    }
}

fn parse_range(part: &str) -> Result<PageRange, Error> {
    let invalid = |reason: &str| Error::InvalidPageSelection(format!("« {part} » {reason}"));
    let number = |text: &str| -> Result<u32, Error> {
        let value: u32 = text
            .trim()
            .parse()
            .map_err(|_| invalid("n'est pas un numéro de page"))?;
        if value == 0 {
            return Err(invalid(": la numérotation commence à 1"));
        }
        Ok(value)
    };

    match part.split_once('-') {
        None => {
            let page = number(part)?;
            Ok(PageRange {
                first: page,
                last: Some(page),
            })
        }
        Some((start, end)) => {
            let first = if start.trim().is_empty() { 1 } else { number(start)? };
            let last = if end.trim().is_empty() {
                None
            } else {
                Some(number(end)?)
            };
            if start.trim().is_empty() && last.is_none() {
                return Err(invalid("est incomplet"));
            }
            if last.is_some_and(|last| last < first) {
                return Err(invalid(": la fin précède le début"));
            }
            Ok(PageRange { first, last })
        }
    }
}

impl fmt::Display for PageSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("toutes"),
            Self::Ranges(ranges) => {
                for (index, range) in ranges.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    match range.last {
                        Some(last) if last == range.first => write!(f, "{}", range.first)?,
                        Some(last) => write!(f, "{}-{last}", range.first)?,
                        None => write!(f, "{}-", range.first)?,
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> PageSelection {
        input.parse().unwrap()
    }

    #[test]
    fn empty_selects_everything() {
        assert_eq!(parse(""), PageSelection::All);
        assert_eq!(parse("  * "), PageSelection::All);
        assert_eq!(parse(" , "), PageSelection::All);
    }

    #[test]
    fn parses_singles_ranges_and_open_ends() {
        let selection = parse("1-3, 7 ; 10-");
        assert!(selection.includes(1) && selection.includes(3));
        assert!(!selection.includes(4));
        assert!(selection.includes(7));
        assert!(!selection.includes(9));
        assert!(selection.includes(10) && selection.includes(10_000));
        assert_eq!(selection.count_in(12), 3 + 1 + 3);
    }

    #[test]
    fn leading_dash_starts_at_first_page() {
        let selection = parse("-2");
        assert!(selection.includes(1) && selection.includes(2) && !selection.includes(3));
    }

    #[test]
    fn rejects_invalid_input() {
        for input in ["0", "a", "3-1", "-", "1-x", "2--3"] {
            assert!(
                matches!(input.parse::<PageSelection>(), Err(Error::InvalidPageSelection(_))),
                "{input} aurait dû être refusé"
            );
        }
    }

    #[test]
    fn display_round_trips() {
        let selection = parse("1-3,7,10-");
        assert_eq!(selection.to_string(), "1-3, 7, 10-");
        assert_eq!(parse(&selection.to_string()), selection);
    }

    #[test]
    fn ensure_matches_reports_empty_intersection() {
        let error = parse("50-60").ensure_matches(10).unwrap_err();
        assert!(matches!(error, Error::NoPageSelected { total: 10, .. }));
        assert_eq!(parse("5-60").ensure_matches(10).unwrap(), 6);
    }
}
