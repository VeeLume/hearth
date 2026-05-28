//! Pure parser for the RSI public citizen page (`/citizens/<handle>`).
//!
//! Input: the raw HTML string of `/en/citizens/<handle>`.
//! Output: a `ProfileInfo` carrying the two truly-immutable anchors
//! (`citizen_record`, `enlisted`) plus convenience fields (current
//! handle, main-org SID + rank).
//!
//! Lives in `hearth-core` so it has no I/O and can be exercised purely
//! in unit tests — the HTTP fetch wrapper lives in src-tauri.
//!
//! The page is rendered server-side and uses the same class anchors
//! that have been in place since at least 2017. The selectors below
//! prefer class hooks unique to the public-profile page; if RSI ever
//! restructures, `ProfileError::MissingField` calls out which anchor
//! disappeared so the fallback (bio-code or manual entry) can step in.
//!
//! Date format note: the page renders `Enlisted` as e.g.
//! `"Jan 31, 2016"`. We normalize to ISO `"YYYY-MM-DD"` so the storage
//! column stays sortable / parseable everywhere else.

use scraper::{ElementRef, Html, Selector};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileInfo {
    /// Current RSI handle exactly as displayed (case preserved).
    pub handle: String,
    /// `UEE Citizen Record` integer (the `#` is stripped).
    pub citizen_record: i64,
    /// ISO date `"YYYY-MM-DD"` parsed from `"Mon DD, YYYY"`.
    pub enlisted: String,
    /// Main organization Spectrum Identification (e.g. `"WISEGUYS"`).
    /// `None` if the citizen has no main org or all orgs are redacted.
    pub primary_org_sid: Option<String>,
    /// Rank label within `primary_org_sid` (e.g. `"Herald"`).
    pub primary_org_rank: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("missing field on profile page: {0}")]
    MissingField(&'static str),
    #[error("could not parse {field}: {detail}")]
    Parse {
        field: &'static str,
        detail: String,
    },
}

/// Parse a `/citizens/<handle>` HTML body into a `ProfileInfo`.
pub fn parse(html: &str) -> Result<ProfileInfo, ProfileError> {
    let doc = Html::parse_document(html);

    let citizen_record = parse_citizen_record(&doc)?;
    let handle = find_label_value(&doc, "Handle name")
        .ok_or(ProfileError::MissingField("Handle name"))?;
    let enlisted_raw = find_label_value(&doc, "Enlisted")
        .ok_or(ProfileError::MissingField("Enlisted"))?;
    let enlisted = normalize_enlisted(&enlisted_raw)?;

    let primary_org_sid = find_label_value(&doc, "Spectrum Identification (SID)")
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("redacted"));
    let primary_org_rank = find_label_value(&doc, "Organization rank")
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("redacted"));

    Ok(ProfileInfo {
        handle,
        citizen_record,
        enlisted,
        primary_org_sid,
        primary_org_rank,
    })
}

/// `<p class="entry citizen-record"><strong class="value">#1196670</strong></p>`
fn parse_citizen_record(doc: &Html) -> Result<i64, ProfileError> {
    let p_sel = Selector::parse("p.citizen-record strong.value").unwrap();
    let raw = doc
        .select(&p_sel)
        .next()
        .map(text)
        .ok_or(ProfileError::MissingField("UEE Citizen Record"))?;
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().map_err(|e| ProfileError::Parse {
        field: "UEE Citizen Record",
        detail: format!("`{raw}` → {e}"),
    })
}

/// Walks every `<p class="entry">` block and returns the trimmed text of
/// the sibling `<strong class="value">` whose `<span class="label">`
/// matches `label` exactly (case-sensitive, trimmed).
///
/// The profile page uses the same label/value pattern for Handle name,
/// Enlisted, Spectrum Identification, Organization rank, Location, etc.,
/// so one walker covers all of them.
fn find_label_value(doc: &Html, label: &str) -> Option<String> {
    let entry_sel = Selector::parse("p.entry").unwrap();
    let label_sel = Selector::parse("span.label").unwrap();
    let value_sel = Selector::parse("strong.value").unwrap();
    for entry in doc.select(&entry_sel) {
        let Some(label_el) = entry.select(&label_sel).next() else {
            continue;
        };
        if text(label_el) != label {
            continue;
        }
        if let Some(value_el) = entry.select(&value_sel).next() {
            return Some(text(value_el));
        }
    }
    None
}

/// `Jan 31, 2016` → `2016-01-31`. RSI always renders the localised
/// English short month + day + year; we don't see other formats in
/// practice but if we do, the error surfaces the raw string.
fn normalize_enlisted(raw: &str) -> Result<String, ProfileError> {
    let trimmed = raw.trim();
    let parsed = chrono::NaiveDate::parse_from_str(trimmed, "%b %d, %Y").map_err(|e| {
        ProfileError::Parse {
            field: "Enlisted",
            detail: format!("`{trimmed}` → {e}"),
        }
    })?;
    Ok(parsed.format("%Y-%m-%d").to_string())
}

fn text(el: ElementRef<'_>) -> String {
    el.text().collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic fixture that mirrors the structure of a live RSI
    // citizen page (verified 2026-05-28 against /en/citizens/VeeLume).
    // Synthetic so no real user data lives in the repo.
    const FIXTURE: &str = r#"
<!doctype html>
<html><body>
  <div class="profile-content overview-content clearfix">
    <p class="entry citizen-record">
      <span class="label">UEE Citizen Record</span>
      <strong class="value">#4242</strong>
    </p>
    <div class="profile left-col">
      <div class="info">
        <p class="entry"><strong class="value">DisplayName</strong></p>
        <p class="entry">
          <span class="label">Handle name</span>
          <strong class="value">TestUser</strong>
        </p>
      </div>
    </div>
    <div class="main-org right-col visibility-V">
      <div class="info">
        <p class="entry">
          <a href="/orgs/EXAMPLE" class="value">Example Org</a>
        </p>
        <p class="entry">
          <span class="label data1">Spectrum Identification (SID)</span>
          <strong class="value data13">EXAMPLE</strong>
        </p>
        <p class="entry">
          <span class="label data9">Organization rank</span>
          <strong class="value data4">Recruit</strong>
        </p>
      </div>
    </div>
    <div class="left-col">
      <div class="inner">
        <p class="entry">
          <span class="label">Enlisted</span>
          <strong class="value">Mar 14, 2015</strong>
        </p>
        <p class="entry">
          <span class="label">Location</span>
          <strong class="value">Germany</strong>
        </p>
      </div>
    </div>
  </div>
</body></html>
"#;

    #[test]
    fn parses_full_profile() {
        let info = parse(FIXTURE).expect("parse");
        assert_eq!(info.handle, "TestUser");
        assert_eq!(info.citizen_record, 4242);
        assert_eq!(info.enlisted, "2015-03-14");
        assert_eq!(info.primary_org_sid.as_deref(), Some("EXAMPLE"));
        assert_eq!(info.primary_org_rank.as_deref(), Some("Recruit"));
    }

    #[test]
    fn parses_orgless_profile() {
        // No main-org block at all → SID + rank both None, anchors still parse.
        let html = r#"
<!doctype html><html><body>
  <p class="entry citizen-record">
    <span class="label">UEE Citizen Record</span>
    <strong class="value">#7</strong>
  </p>
  <p class="entry">
    <span class="label">Handle name</span>
    <strong class="value">Loner</strong>
  </p>
  <p class="entry">
    <span class="label">Enlisted</span>
    <strong class="value">Dec 1, 2012</strong>
  </p>
</body></html>"#;
        let info = parse(html).expect("parse");
        assert_eq!(info.handle, "Loner");
        assert_eq!(info.citizen_record, 7);
        assert_eq!(info.enlisted, "2012-12-01");
        assert!(info.primary_org_sid.is_none());
        assert!(info.primary_org_rank.is_none());
    }

    #[test]
    fn redacted_org_fields_become_none() {
        let html = r#"
<!doctype html><html><body>
  <p class="entry citizen-record"><span class="label">UEE Citizen Record</span><strong class="value">#1</strong></p>
  <p class="entry"><span class="label">Handle name</span><strong class="value">A</strong></p>
  <p class="entry"><span class="label">Enlisted</span><strong class="value">Jan 1, 2020</strong></p>
  <p class="entry"><span class="label">Spectrum Identification (SID)</span><strong class="value">REDACTED</strong></p>
</body></html>"#;
        let info = parse(html).expect("parse");
        assert!(info.primary_org_sid.is_none());
    }

    #[test]
    fn missing_handle_errors() {
        let html = r#"<!doctype html><html><body>
  <p class="entry citizen-record"><span class="label">UEE Citizen Record</span><strong class="value">#1</strong></p>
  <p class="entry"><span class="label">Enlisted</span><strong class="value">Jan 1, 2020</strong></p>
</body></html>"#;
        let err = parse(html).unwrap_err();
        assert!(matches!(err, ProfileError::MissingField("Handle name")));
    }
}
