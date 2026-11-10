//! Times and dates written the way a locale writes them.
//!
//! The patterns are CLDR's, through ICU4X: the locale decides the order of
//! the fields, 12- or 24-hour time, the separators, the digits and the
//! calendar (a Thai locale counts years in the Buddhist era). The time is
//! the civil time of the [`Zoned`] value given, so the time zone and its
//! rules have been applied already; no zone name is written.

use icu_datetime::fieldsets::{T, YMD, YMDE, YMDT};
use icu_datetime::input::DateTime;
use icu_datetime::{DateTimeFormatter, DateTimeFormatterPreferences};
use icu_locale_core::Locale;
use jiff::Zoned;
use jiff_icu::ConvertFrom as _;

use crate::boundary::Precision;
use crate::error::FormatError;

/// How much of a date to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Length {
    /// Month names in full: "10 November 2026".
    Long,
    /// Abbreviated month names: "10 Nov 2026".
    Medium,
    /// Numbers only: "10/11/2026".
    Short,
}

/// What a [`Formatter`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    /// The time of day, to the minute or the second: "14:05".
    Time(Precision),
    /// The date: year, month and day.
    Date(Length),
    /// The date with the day of the week.
    WeekdayDate(Length),
    /// The date and the time of day.
    DateTime(Length, Precision),
}

/// Formats times in one locale and [`Style`].
#[derive(Debug)]
pub struct Formatter {
    inner: Inner,
}

#[derive(Debug)]
enum Inner {
    Time(DateTimeFormatter<T>),
    Date(DateTimeFormatter<YMD>),
    WeekdayDate(DateTimeFormatter<YMDE>),
    DateTime(DateTimeFormatter<YMDT>),
}

impl Formatter {
    /// A formatter for `style` in `locale`, such as the one
    /// [`Environment::locale`](crate::Environment::locale) gives. A locale
    /// CLDR has no data for falls back to its parent, and in the end to the
    /// root locale.
    ///
    /// # Errors
    ///
    /// [`FormatError`] when the patterns for the locale cannot be loaded.
    pub fn new(locale: &Locale, style: Style) -> Result<Self, FormatError> {
        let prefs = DateTimeFormatterPreferences::from(locale);
        let inner = match style {
            Style::Time(precision) => {
                let fields = match precision {
                    Precision::Minutes => T::hm(),
                    Precision::Seconds => T::hms(),
                };
                Inner::Time(DateTimeFormatter::try_new(prefs, fields)?)
            }
            Style::Date(length) => Inner::Date(DateTimeFormatter::try_new(prefs, ymd(length))?),
            Style::WeekdayDate(length) => {
                let fields = match length {
                    Length::Long => YMDE::long(),
                    Length::Medium => YMDE::medium(),
                    Length::Short => YMDE::short(),
                };
                Inner::WeekdayDate(DateTimeFormatter::try_new(prefs, fields)?)
            }
            Style::DateTime(length, precision) => {
                let date = ymd(length);
                let fields = match precision {
                    Precision::Minutes => date.with_time_hm(),
                    Precision::Seconds => date.with_time_hms(),
                };
                Inner::DateTime(DateTimeFormatter::try_new(prefs, fields)?)
            }
        };
        Ok(Self { inner })
    }

    /// `time`, written in the formatter's locale and style.
    #[must_use]
    pub fn format(&self, time: &Zoned) -> String {
        let civil = DateTime::convert_from(time.datetime());
        match &self.inner {
            Inner::Time(f) => f.format(&civil).to_string(),
            Inner::Date(f) => f.format(&civil).to_string(),
            Inner::WeekdayDate(f) => f.format(&civil).to_string(),
            Inner::DateTime(f) => f.format(&civil).to_string(),
        }
    }
}

fn ymd(length: Length) -> YMD {
    match length {
        Length::Long => YMD::long(),
        Length::Medium => YMD::medium(),
        Length::Short => YMD::short(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::posix_to_unicode;
    use crate::test_zones::named;

    /// 2026-11-10 14:05:09 in Berlin.
    fn afternoon() -> Zoned {
        "2026-11-10T13:05:09Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .to_zoned(named("Europe/Berlin"))
    }

    fn write(posix: &str, style: Style) -> String {
        Formatter::new(&posix_to_unicode(posix), style)
            .unwrap()
            .format(&afternoon())
    }

    #[test]
    fn writes_hours_and_minutes_by_the_locale() {
        let hm = Style::Time(Precision::Minutes);
        // CLDR separates the day period with a narrow no-break space.
        assert_eq!(write("en_US.UTF-8", hm), "2:05\u{202f}PM");
        assert_eq!(write("en_GB.UTF-8", hm), "14:05");
        assert_eq!(write("de_DE.UTF-8", hm), "14:05");
        assert_eq!(write("ko_KR.UTF-8", hm), "오후 2:05");
        assert_eq!(write("ar_EG.UTF-8", hm), "٢:٠٥ م");
        assert_eq!(write("C", hm), "14:05");
    }

    #[test]
    fn writes_seconds_when_asked() {
        let hms = Style::Time(Precision::Seconds);
        assert_eq!(write("de_DE.UTF-8", hms), "14:05:09");
        assert_eq!(write("en_US.UTF-8", hms), "2:05:09\u{202f}PM");
    }

    #[test]
    fn writes_dates_in_the_locale_order_and_calendar() {
        assert_eq!(
            write("de_DE.UTF-8", Style::Date(Length::Long)),
            "10. November 2026"
        );
        assert_eq!(write("en_US.UTF-8", Style::Date(Length::Short)), "11/10/26");
        assert_eq!(
            write("ja_JP.UTF-8", Style::Date(Length::Long)),
            "2026年11月10日"
        );
        // The Thai locale's calendar is the Buddhist one: 2026 is 2569 BE.
        assert!(write("th_TH.UTF-8", Style::Date(Length::Long)).contains("2569"));
        assert_eq!(
            write("fr_FR.UTF-8", Style::WeekdayDate(Length::Long)),
            "mardi 10 novembre 2026"
        );
        assert_eq!(
            write(
                "en_GB.UTF-8",
                Style::DateTime(Length::Medium, Precision::Minutes)
            ),
            "10 Nov 2026, 14:05"
        );
    }

    #[test]
    fn can_format_on_another_thread() {
        fn shareable<T: Send + Sync>() {}
        shareable::<Formatter>();
    }

    #[test]
    fn uses_the_zones_civil_time() {
        let tokyo = afternoon().with_time_zone(named("Asia/Tokyo"));
        let f =
            Formatter::new(&posix_to_unicode("de_DE"), Style::Time(Precision::Minutes)).unwrap();
        assert_eq!(f.format(&tokyo), "22:05");
    }
}
