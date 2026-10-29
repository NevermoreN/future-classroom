//! Local civil time only: the platform adapter owns timezone conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeError {
    InvalidDate,
    InvalidMinute,
    InvalidSecond,
    InvalidRange,
    IncompleteInput,
    InvalidDigit,
    InvalidCursor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalDate {
    year: i32,
    month: u8,
    day: u8,
}
impl LocalDate {
    pub fn new(year: i32, month: u8, day: u8) -> Result<Self, TimeError> {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            4 | 6 | 9 | 11 => 30,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => return Err(TimeError::InvalidDate),
        };
        if !(1..=9999).contains(&year) || day == 0 || day > days {
            return Err(TimeError::InvalidDate);
        }
        Ok(Self { year, month, day })
    }
    pub const fn year(self) -> i32 {
        self.year
    }
    pub const fn month(self) -> u8 {
        self.month
    }
    pub const fn day(self) -> u8 {
        self.day
    }
    /// Gregorian day count, relative to a private epoch; valid across leap years.
    pub fn day_number(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy
    }
    pub fn days_since(self, earlier: Self) -> i64 {
        self.day_number() - earlier.day_number()
    }
    pub fn weekday(self) -> Weekday {
        // 0001-01-01 was Monday; March-based epoch has offset 306.
        Weekday::from_monday_index((self.day_number() - 306).rem_euclid(7) as u8).unwrap()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}
impl Weekday {
    pub fn from_monday_index(index: u8) -> Option<Self> {
        Some(match index {
            0 => Self::Monday,
            1 => Self::Tuesday,
            2 => Self::Wednesday,
            3 => Self::Thursday,
            4 => Self::Friday,
            5 => Self::Saturday,
            6 => Self::Sunday,
            _ => return None,
        })
    }
    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Minute(u16);
impl Minute {
    pub fn new(value: u16) -> Result<Self, TimeError> {
        if value < 1440 {
            Ok(Self(value))
        } else {
            Err(TimeError::InvalidMinute)
        }
    }
    pub fn from_hm(hour: u8, minute: u8) -> Result<Self, TimeError> {
        if hour > 23 || minute > 59 {
            return Err(TimeError::InvalidMinute);
        }
        Self::new(u16::from(hour) * 60 + u16::from(minute))
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MinuteRange {
    pub begin: Minute,
    pub end: Minute,
}
impl MinuteRange {
    pub fn new(begin: Minute, end: Minute) -> Result<Self, TimeError> {
        if begin >= end {
            Err(TimeError::InvalidRange)
        } else {
            Ok(Self { begin, end })
        }
    }
    pub fn contains_second(self, second: u32) -> bool {
        second >= u32::from(self.begin.get()) * 60 && second < u32::from(self.end.get()) * 60
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.begin < other.end && other.begin < self.end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalDateTime {
    pub date: LocalDate,
    second_of_day: u32,
}
impl LocalDateTime {
    pub fn new(date: LocalDate, second_of_day: u32) -> Result<Self, TimeError> {
        if second_of_day >= 86400 {
            Err(TimeError::InvalidSecond)
        } else {
            Ok(Self {
                date,
                second_of_day,
            })
        }
    }
    pub const fn second_of_day(self) -> u32 {
        self.second_of_day
    }
}

/// Eight digit positions; punctuation is rendered, never stored as editable text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimeRangeInput {
    digits: [Option<u8>; 8],
    cursor: usize,
    selection: Option<(usize, usize)>,
}
impl TimeRangeInput {
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn set_cursor(&mut self, cursor: usize) -> Result<(), TimeError> {
        if cursor > 8 {
            return Err(TimeError::InvalidCursor);
        }
        self.cursor = cursor;
        self.selection = None;
        Ok(())
    }
    pub fn select(&mut self, start: usize, end: usize) -> Result<(), TimeError> {
        if start >= end || end > 8 {
            return Err(TimeError::InvalidCursor);
        }
        self.cursor = start;
        self.selection = Some((start, end));
        Ok(())
    }
    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        self.selection = None;
    }
    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(8);
        self.selection = None;
    }
    fn clear_selection(&mut self) -> bool {
        if let Some((start, end)) = self.selection.take() {
            self.digits[start..end].fill(None);
            self.cursor = start;
            true
        } else {
            false
        }
    }
    pub fn insert(&mut self, value: char) -> Result<(), TimeError> {
        let digit = normalized_digit(value).ok_or(TimeError::InvalidDigit)?;
        self.clear_selection();
        if self.cursor == 8 {
            return Err(TimeError::InvalidCursor);
        }
        self.digits[self.cursor] = Some(digit);
        self.cursor += 1;
        Ok(())
    }
    pub fn paste(&mut self, value: &str) -> Result<(), TimeError> {
        let values: Vec<_> = value.chars().take(9).map(normalized_digit).collect();
        if values.len() != 8 || values.iter().any(Option::is_none) {
            return Err(TimeError::InvalidDigit);
        }
        for (slot, digit) in self.digits.iter_mut().zip(values) {
            *slot = digit;
        }
        self.cursor = 8;
        self.selection = None;
        Ok(())
    }
    pub fn backspace(&mut self) {
        if self.clear_selection() {
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
            self.digits[self.cursor] = None;
        }
    }
    pub fn delete(&mut self) {
        if self.clear_selection() {
            return;
        }
        if self.cursor < 8 {
            self.digits[self.cursor] = None;
        }
    }
    pub fn display(&self) -> String {
        let d: Vec<char> = self
            .digits
            .iter()
            .map(|v| v.map_or('X', |n| char::from(b'0' + n)))
            .collect();
        format!(
            "{}{}：{}{}-{}{}：{}{}",
            d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
        )
    }
    pub fn finish(&self) -> Result<MinuteRange, TimeError> {
        let mut d = [0; 8];
        for (dst, src) in d.iter_mut().zip(self.digits) {
            *dst = src.ok_or(TimeError::IncompleteInput)?;
        }
        MinuteRange::new(
            Minute::from_hm(d[0] * 10 + d[1], d[2] * 10 + d[3])?,
            Minute::from_hm(d[4] * 10 + d[5], d[6] * 10 + d[7])?,
        )
    }
}
fn normalized_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        '０'..='９' => Some((c as u32 - '０' as u32) as u8),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gregorian_dates_and_weekdays() {
        assert!(LocalDate::new(1900, 2, 29).is_err());
        assert!(LocalDate::new(2000, 2, 29).is_ok());
        let mon = LocalDate::new(2026, 10, 26).unwrap();
        assert_eq!(mon.weekday(), Weekday::Monday);
        assert_eq!(LocalDate::new(2026, 11, 2).unwrap().days_since(mon), 7);
        assert_eq!(
            LocalDate::new(2027, 1, 1)
                .unwrap()
                .days_since(LocalDate::new(2026, 12, 31).unwrap()),
            1
        );
    }
    #[test]
    fn mask_edits_and_atomic_paste() {
        let mut input = TimeRangeInput::default();
        input.paste("０８２００９００").unwrap();
        assert_eq!(input.display(), "08：20-09：00");
        assert_eq!(input.finish().unwrap().begin.get(), 500);
        let before = input.clone();
        assert!(input.paste("2400oops").is_err());
        assert_eq!(input, before);
        input.backspace();
        assert_eq!(input.finish(), Err(TimeError::IncompleteInput));
        input.insert('0').unwrap();
        input.set_cursor(0).unwrap();
        input.delete();
        assert_eq!(input.display(), "X8：20-09：00");
    }
    #[test]
    fn ranges_reject_midnight_and_use_exclusive_end() {
        for value in ["23002400", "09000820", "23602359", "09000900"] {
            let mut i = TimeRangeInput::default();
            i.paste(value).unwrap();
            assert!(i.finish().is_err());
        }
        let mut i = TimeRangeInput::default();
        i.paste("08200900").unwrap();
        assert!(i.finish().unwrap().contains_second(32399));
        assert!(!i.finish().unwrap().contains_second(32400));
    }
    #[test]
    fn segment_selection_replaces_digits_without_editable_punctuation() {
        let mut input = TimeRangeInput::default();
        input.paste("08200900").unwrap();
        input.select(2, 4).unwrap();
        input.insert('3').unwrap();
        input.insert('0').unwrap();
        assert_eq!(input.display(), "08：30-09：00");
        input.move_left();
        assert_eq!(input.cursor(), 3);
        input.move_right();
        assert_eq!(input.cursor(), 4);
        input.select(4, 8).unwrap();
        input.backspace();
        assert_eq!(input.display(), "08：30-XX：XX");
        assert_eq!(input.finish(), Err(TimeError::IncompleteInput));
        assert!(input.select(8, 9).is_err());
    }
}
