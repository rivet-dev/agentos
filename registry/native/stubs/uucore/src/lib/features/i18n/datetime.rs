// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore fieldsets prefs febr

//! Locale-aware datetime formatting utilities using ICU and jiff-icu

use icu_calendar::Date;
use icu_calendar::cal::{Buddhist, Ethiopian, Iso, Persian};
use icu_datetime::DateTimeFormatter;
use icu_datetime::fieldsets;
use icu_locale::Locale;
use jiff::civil::Date as JiffDate;
use jiff_icu::ConvertFrom;
use std::sync::OnceLock;

use crate::i18n::get_locale_from_env;

#[derive(Default)]
struct StrftimeReplacements {
    year: Option<String>,
    month: Option<String>,
    day_zero_padded: Option<String>,
    day_space_padded: Option<String>,
    month_long: Option<String>,
    month_abbrev: Option<String>,
    weekday_long: Option<String>,
    weekday_short: Option<String>,
}

/// Get the locale for time/date formatting from LC_TIME environment variable
pub fn get_time_locale() -> &'static (Locale, super::UEncoding) {
    static TIME_LOCALE: OnceLock<(Locale, super::UEncoding)> = OnceLock::new();

    TIME_LOCALE.get_or_init(|| get_locale_from_env("LC_TIME"))
}

/// Check if we should use ICU for locale-aware time/date formatting
///
/// Returns true for non-C/POSIX locales, false otherwise
pub fn should_use_icu_locale() -> bool {
    use icu_locale::locale;

    let (locale, _encoding) = get_time_locale();

    // Use ICU for non-default locales (anything other than C/POSIX)
    // The default locale is "und" (undefined) representing C/POSIX
    *locale != locale!("und")
}

/// Determine the appropriate calendar system for a given locale
pub fn get_locale_calendar_type(locale: &Locale) -> CalendarType {
    let locale_str = locale.to_string();

    match locale_str.as_str() {
        // Thai locales use Buddhist calendar
        s if s.starts_with("th") => CalendarType::Buddhist,
        // Persian/Farsi locales use Persian calendar (Solar Hijri)
        s if s.starts_with("fa") => CalendarType::Persian,
        // Amharic (Ethiopian) locales use Ethiopian calendar
        s if s.starts_with("am") => CalendarType::Ethiopian,
        // Default to Gregorian for all other locales
        _ => CalendarType::Gregorian,
    }
}

/// Calendar types supported for locale-aware formatting
#[derive(Debug, Clone, PartialEq)]
pub enum CalendarType {
    /// Gregorian calendar (used by most locales)
    Gregorian,
    /// Buddhist calendar (Thai locales) - adds 543 years to Gregorian year
    Buddhist,
    /// Persian Solar Hijri calendar (Persian/Farsi locales) - subtracts 621/622 years
    Persian,
    /// Ethiopian calendar (Amharic locales) - subtracts 7/8 years
    Ethiopian,
}

/// Transform a strftime format string to use locale-specific calendar values
pub fn localize_format_string(format: &str, date: JiffDate) -> String {
    let (locale, _) = get_time_locale();
    let iso_date = Date::<Iso>::convert_from(date);

    let mut replacements = StrftimeReplacements::default();

    // For non-Gregorian calendars, replace date components with converted values
    let calendar_type = get_locale_calendar_type(locale);
    if calendar_type != CalendarType::Gregorian {
        let (cal_year, cal_month, cal_day) = match calendar_type {
            CalendarType::Buddhist => {
                let d = iso_date.to_calendar(Buddhist);
                (d.extended_year(), d.month().ordinal, d.day_of_month().0)
            }
            CalendarType::Persian => {
                let d = iso_date.to_calendar(Persian);
                (d.extended_year(), d.month().ordinal, d.day_of_month().0)
            }
            CalendarType::Ethiopian => {
                let d = iso_date.to_calendar(Ethiopian::new());
                (d.extended_year(), d.month().ordinal, d.day_of_month().0)
            }
            CalendarType::Gregorian => unreachable!(),
        };
        replacements.year = Some(cal_year.to_string());
        replacements.month = Some(format!("{cal_month:02}"));
        replacements.day_zero_padded = Some(format!("{cal_day:02}"));
        replacements.day_space_padded = Some(format!("{cal_day:2}"));
    }

    // Format localized names using ICU DateTimeFormatter
    let locale_prefs = locale.clone().into();

    if format.contains("%B") {
        if let Ok(f) = DateTimeFormatter::try_new(locale_prefs, fieldsets::M::long()) {
            replacements.month_long = Some(f.format(&iso_date).to_string());
        }
    }
    if format.contains("%b") || format.contains("%h") {
        if let Ok(f) = DateTimeFormatter::try_new(locale_prefs, fieldsets::M::medium()) {
            // ICU's medium format may include trailing periods (e.g., "febr." for Hungarian),
            // which when combined with locale format strings that also add periods after
            // %b (e.g., "%Y. %b. %d") results in double periods ("febr..").
            // The standard C/POSIX locale via nl_langinfo returns abbreviations
            // WITHOUT trailing periods, so we strip them here for consistency.
            let month_abbrev = f.format(&iso_date).to_string();
            let month_abbrev = month_abbrev.trim_end_matches('.').to_string();
            replacements.month_abbrev = Some(month_abbrev);
        }
    }
    if format.contains("%A") {
        if let Ok(f) = DateTimeFormatter::try_new(locale_prefs, fieldsets::E::long()) {
            replacements.weekday_long = Some(f.format(&iso_date).to_string());
        }
    }
    if format.contains("%a") {
        if let Ok(f) = DateTimeFormatter::try_new(locale_prefs, fieldsets::E::short()) {
            replacements.weekday_short = Some(f.format(&iso_date).to_string());
        }
    }

    replace_strftime_components(format, &replacements)
}

fn replace_strftime_components(format: &str, replacements: &StrftimeReplacements) -> String {
    let mut replaced = String::with_capacity(format.len());
    let mut chars = format.chars();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            replaced.push(ch);
            continue;
        }

        let Some(next) = chars.next() else {
            replaced.push('%');
            break;
        };

        match next {
            '%' => replaced.push_str("%%"),
            'Y' => push_replacement_or_directive(&mut replaced, next, replacements.year.as_deref()),
            'm' => {
                push_replacement_or_directive(&mut replaced, next, replacements.month.as_deref())
            }
            'd' => push_replacement_or_directive(
                &mut replaced,
                next,
                replacements.day_zero_padded.as_deref(),
            ),
            'e' => push_replacement_or_directive(
                &mut replaced,
                next,
                replacements.day_space_padded.as_deref(),
            ),
            'B' => {
                push_replacement_or_directive(
                    &mut replaced,
                    next,
                    replacements.month_long.as_deref(),
                );
            }
            'b' | 'h' => {
                push_replacement_or_directive(
                    &mut replaced,
                    next,
                    replacements.month_abbrev.as_deref(),
                );
            }
            'A' => {
                push_replacement_or_directive(
                    &mut replaced,
                    next,
                    replacements.weekday_long.as_deref(),
                );
            }
            'a' => {
                push_replacement_or_directive(
                    &mut replaced,
                    next,
                    replacements.weekday_short.as_deref(),
                );
            }
            _ => push_replacement_or_directive(&mut replaced, next, None),
        }
    }

    replaced
}

fn push_replacement_or_directive(
    replaced: &mut String,
    directive: char,
    replacement: Option<&str>,
) {
    if let Some(replacement) = replacement {
        replaced.push_str(replacement);
    } else {
        replaced.push('%');
        replaced.push(directive);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calendar_type_detection() {
        use icu_locale::locale;
        assert_eq!(
            get_locale_calendar_type(&locale!("th-TH")),
            CalendarType::Buddhist
        );
        assert_eq!(
            get_locale_calendar_type(&locale!("fa-IR")),
            CalendarType::Persian
        );
        assert_eq!(
            get_locale_calendar_type(&locale!("am-ET")),
            CalendarType::Ethiopian
        );
        assert_eq!(
            get_locale_calendar_type(&locale!("en-US")),
            CalendarType::Gregorian
        );
    }

    #[test]
    fn test_replace_strftime_components_preserves_escaped_percent() {
        let replacements = StrftimeReplacements {
            year: Some("2026".to_string()),
            month: Some("06".to_string()),
            day_zero_padded: Some("07".to_string()),
            month_long: Some("June".to_string()),
            month_abbrev: Some("Jun".to_string()),
            weekday_long: Some("Sunday".to_string()),
            weekday_short: Some("Sun".to_string()),
            ..Default::default()
        };

        assert_eq!(
            "2026-%%m-07-June-Jun-Jun-Sunday-Sun %% %q % %%B %%a",
            replace_strftime_components("%Y-%%m-%d-%B-%b-%h-%A-%a %% %q % %%B %%a", &replacements,)
        );
    }

    #[test]
    fn test_replace_strftime_components_preserves_literal_nuls() {
        let replacements = StrftimeReplacements {
            year: Some("2026".to_string()),
            ..Default::default()
        };

        assert_eq!(
            "\0\02026",
            replace_strftime_components("\0\0%Y", &replacements)
        );
    }
}
