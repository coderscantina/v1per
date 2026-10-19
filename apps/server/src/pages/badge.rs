//! Flat SVG badges for a page's status and uptime, in the page's default locale.

use v1per_protocol::status_page::{Locale, PageStatus};

use super::text;

const LABEL_COLOR: &str = "#555";
const NO_DATA_COLOR: &str = "#8a8f98";

fn status_color(status: Option<PageStatus>) -> &'static str {
    match status {
        Some(PageStatus::Operational) => "#2f9e5b",
        Some(PageStatus::Maintenance) => "#3b7ddd",
        Some(PageStatus::Degraded) => "#c99a06",
        Some(PageStatus::PartialOutage) => "#e0700b",
        Some(PageStatus::MajorOutage) => "#d33a2c",
        None => NO_DATA_COLOR,
    }
}

pub(super) fn status(locale: Locale, status: Option<PageStatus>) -> String {
    svg(
        text::status_label(locale),
        text::status(locale, status),
        status_color(status),
    )
}

/// `window` is `24h`, `30d` or `90d`; `uptime` a ratio.
pub(super) fn uptime(locale: Locale, window: &str, uptime: Option<f64>) -> String {
    let value = match uptime {
        Some(uptime) => {
            let percent = format!("{:.2}", uptime * 100.0);
            match locale {
                Locale::En => format!("{percent}%"),
                Locale::De => format!("{} %", percent.replace('.', ",")),
            }
        }
        None => text::status(locale, None).to_owned(),
    };
    let color = match uptime {
        Some(u) if u >= 0.999 => status_color(Some(PageStatus::Operational)),
        Some(u) if u >= 0.99 => status_color(Some(PageStatus::Degraded)),
        Some(u) if u >= 0.95 => status_color(Some(PageStatus::PartialOutage)),
        Some(_) => status_color(Some(PageStatus::MajorOutage)),
        None => NO_DATA_COLOR,
    };
    svg(&text::uptime_label(locale, window), &value, color)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Text width at 11 px Verdana, estimated per character.
fn width(text: &str) -> u32 {
    text.chars().count() as u32 * 7 + 10
}

fn svg(label: &str, value: &str, color: &str) -> String {
    let (lw, vw) = (width(label), width(value));
    let total = lw + vw;
    let (label, value) = (escape(label), escape(value));
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="20" role="img" aria-label="{label}: {value}"><title>{label}: {value}</title><clipPath id="r"><rect width="{total}" height="20" rx="3" fill="#fff"/></clipPath><g clip-path="url(#r)"><rect width="{lw}" height="20" fill="{LABEL_COLOR}"/><rect x="{lw}" width="{vw}" height="20" fill="{color}"/></g><g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11"><text x="{lx}" y="14">{label}</text><text x="{vx}" y="14">{value}</text></g></svg>"##,
        lx = lw / 2,
        vx = lw + vw / 2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges_escape_and_localize() {
        let badge = status(Locale::De, Some(PageStatus::MajorOutage));
        assert!(badge.starts_with("<svg") && badge.contains("schwerer Ausfall"));
        assert!(uptime(Locale::En, "24h", Some(0.99951)).contains(">99.95%<"));
        assert!(uptime(Locale::De, "90d", Some(1.0)).contains(">100,00 %<"));
        assert!(svg("a<b", "\"&", "#000").contains("a&lt;b: &quot;&amp;"));
    }
}
