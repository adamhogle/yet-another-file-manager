//! Group-based access control (ADR-0005): the config-driven allow/deny grants,
//! the gitignore-style deny-pattern matcher, the evaluation function and the
//! `--check-access` report. The config's group names filter the user's groups
//! claim; the visible root is the union of the global allow list and the allow
//! entries of the user's groups, minus every matching deny.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// The validated `access` config block. This is the single runtime shape; the
/// parse-time shape lives in `AccessConfigFile` (validated by
/// `validate_access_config`).
///
/// Semantics (one sentence): a user's visible root is the union of
/// `global_allow` and the allow entries of their groups, inside the shared
/// root, minus every matching `global_deny` and every allow-specific deny
/// within its allow's scope. Order does not matter (set semantics), no
/// negation operator exists, and a denied path hides everything underneath
/// it.
#[derive(Debug, Clone)]
pub struct AccessConfig {
    /// The global baseline: relative paths (normalized, `""` is the root
    /// itself) every user sees regardless of group membership.
    global_allow: Vec<String>,
    /// Deny patterns applying to every user and every allow path.
    global_deny: Vec<DenyPattern>,
    /// Grants mapping a configured authentik group to its delete capability
    /// and allow entries. The user's groups claim is filtered to these names;
    /// a group in the claim that the config does not name is ignored
    /// entirely.
    grants: BTreeMap<String, Grant>,
}

/// One allow entry inside a grant: the allowed directory path plus the
/// optional allow-specific denies scoped to that path (for example `*.nfo`
/// denied under `/tv-shows` while other paths keep `.nfo` visible).
#[derive(Debug, Clone)]
pub struct AllowEntry {
    /// The normalized relative path of the allowed directory (`""` is the
    /// root itself, covering everything). Allows take no wildcards: exposing
    /// a subtree means naming its parent.
    path: String,
    /// Deny patterns applying only under this allow's path.
    deny: Vec<DenyPattern>,
}

/// One validated grant: the group's all-or-nothing delete capability plus
/// its allow entries.
#[derive(Debug, Clone)]
pub struct Grant {
    /// Whether members of this group may delete visible files (ADR-0006).
    /// All or nothing: no per-path delete rules exist. Files visible only
    /// through the global baseline are never deletable.
    delete: bool,
    allow: Vec<AllowEntry>,
}

/// Parse-time shape of the `access` block. Kept optional inside the config
/// file shape so test configs without the block can still parse; the
/// startup-abort decision for a missing block lives in `initialize_access`.
#[derive(Debug, Deserialize)]
pub struct AccessConfigFile {
    #[serde(rename = "allow", default)]
    allow: Vec<String>,
    #[serde(rename = "deny", default)]
    deny: Vec<String>,
    #[serde(rename = "grants", default)]
    grants: BTreeMap<String, GrantFile>,
}

/// Parse-time shape of one grant: the group's all-or-nothing delete
/// capability plus its allow entries, each with an optional paired
/// allow-specific deny list.
#[derive(Debug, Deserialize)]
pub struct GrantFile {
    #[serde(rename = "delete", default)]
    delete: Option<bool>,
    #[serde(rename = "allow", default)]
    allow: Vec<AllowEntryFile>,
}

/// Parse-time shape of one allow entry: the paired `path` + `deny`.
#[derive(Debug, Deserialize)]
pub struct AllowEntryFile {
    #[serde(rename = "path")]
    path: String,
    #[serde(rename = "deny", default)]
    deny: Vec<String>,
}

/// A compiled deny pattern: gitignore glob semantics, matched against the
/// normalized path segments of a user-controlled request path. Allows take no
/// wildcards, so only denies compile.
///
/// Matching rules: a pattern containing a `/` (after leading normalization)
/// is anchored to the shared root and matches a prefix of the path's segments
/// starting at the first segment, so a denied directory hides everything
/// underneath it. A pattern without a `/` matches a single segment at any
/// depth, so a bare name hides that file or directory wherever it appears.
/// `*` matches anything except `/` within one segment, `**` matches across
/// segments (zero or more as a middle segment, one or more as a trailing
/// segment), and `?` matches one character. Matching is case-sensitive: the
/// filesystem is case-sensitive and the expectation is exact rules.
#[derive(Debug, Clone)]
pub struct DenyPattern {
    /// The raw pattern text, for the `--check-access` report.
    raw: String,
    /// Whether the normalized pattern contains a `/` (anchored to the root).
    anchored: bool,
    segments: Vec<SegmentMatcher>,
}

/// One segment of a compiled deny pattern.
#[derive(Debug, Clone)]
struct SegmentMatcher {
    /// The segment's glob text (`*` and `?` allowed).
    pattern: String,
    /// Whether the segment is `**`.
    double_star: bool,
}

/// What the evaluation decided, with the reason the `--check-access` report
/// prints. The runtime enforcement reads only `visible`.
#[derive(Debug, Clone)]
/// The access decision for a path: visible with the allow that made it so,
/// or hidden with the reason. The variants make the invariant structural:
/// a visible decision always names its allow source and a hidden one always
/// carries its denial.
pub enum AccessDecision {
    /// The path is inside the visible root. The allow source names which
    /// allow made it visible, for the report: `"global allow"` or
    /// `allow "/path" (grant <group>)`.
    Visible { allow_source: String },
    /// The path is outside the visible root: no allow entry matches, or a
    /// deny pattern matched.
    Hidden(AccessDenial),
}

/// Why a path is hidden for a user.
#[derive(Debug, Clone)]
pub enum AccessDenial {
    /// No allow entry covers the path (deny-by-default).
    NoAllow,
    /// A deny pattern matched the path.
    Denied { pattern: String, source: String },
}

/// Initialize the process-global access state from the validated config.
/// Must be called after `initialize_app_config` (the initializer reads the
/// config through it). A missing `access` block is a startup error because
/// the backend refuses to change what users can see silently (ADR-0005); the
/// debug-gated `YAFM_DISABLE_AUTH` path skips this initializer alongside the
/// oidc requirement, preserving the test binaries' behavior.
pub fn initialize_access() -> Result<(), String> {
    get_access_config().ok_or_else(|| {
        "Configuration access is required; the backend refuses to start without an access configuration."
            .to_string()
    })?;
    Ok(())
}

/// The access context the gate middleware evaluates once per request and
/// stores in request extensions (ADR-0005 decision 7): the scenario's groups
/// and the access config the handlers filter entries with. Handlers read this
/// context instead of consulting the global config independently, so the
/// access evaluation cannot drift between the endpoints.
#[derive(Clone)]
pub struct AccessContext {
    pub groups: Vec<String>,
    access: &'static AccessConfig,
}

impl AccessContext {
    /// Builds the context from the gate's verified identity and the
    /// initialized access config.
    pub fn new(access: &'static AccessConfig, groups: Vec<String>) -> Self {
        Self { groups, access }
    }

    /// The access config backing this context; the field is private so
    /// handlers cannot swap the config the gate evaluated with.
    pub fn access(&self) -> &'static AccessConfig {
        self.access
    }
}

/// Read access to the initialized access config for the access gate
/// middleware and the data handlers. None until initialized; the
/// uninitialized case is unreachable in production (the startup initializer
/// aborts) and the test-only Disabled auth state is the only None case (no
/// enforcement by design, matching the pre-access behavior).
pub fn get_access_config() -> Option<&'static AccessConfig> {
    crate::get_access_state_config()
}

/// Validates the parsed `access` block into the runtime shape. Only called
/// when the block is present; the startup-abort decision for a missing block
/// lives in `initialize_access` so test configs without the block can still
/// parse.
pub fn validate_access_config(block: AccessConfigFile) -> Result<AccessConfig, String> {
    let mut global_allow = Vec::new();
    for raw in &block.allow {
        global_allow.push(validate_allow_path(raw)?);
    }
    if let Some(duplicate) = find_duplicates(&block.allow) {
        return Err(format!(
            "Configuration access allow lists \"{duplicate}\" more than once."
        ));
    }

    let mut global_deny = Vec::new();
    for raw in &block.deny {
        global_deny.push(validate_deny_pattern(raw)?);
    }
    if let Some(duplicate) = find_duplicates(&block.deny) {
        return Err(format!(
            "Configuration access deny lists \"{duplicate}\" more than once."
        ));
    }

    let mut grants = BTreeMap::new();
    for (group, grant) in block.grants {
        let group_name = group.trim().to_string();
        if group_name.is_empty() {
            return Err(
                "Configuration access grants group names must be non-empty strings.".to_string(),
            );
        }
        let mut entries = Vec::new();
        for entry in grant.allow {
            let path = validate_allow_path(&entry.path)?;
            let mut deny = Vec::new();
            for raw in &entry.deny {
                deny.push(validate_deny_pattern(raw)?);
            }
            entries.push(AllowEntry { path, deny });
        }
        if entries.is_empty() {
            return Err(format!(
                "Configuration access grants \"{group_name}\" must list at least one allow entry."
            ));
        }
        grants.insert(
            group_name,
            Grant {
                delete: grant.delete.unwrap_or(false),
                allow: entries,
            },
        );
    }

    Ok(AccessConfig {
        global_allow,
        global_deny,
        grants,
    })
}

/// The first value that appears more than once, for the duplicate checks.
fn find_duplicates(values: &[String]) -> Option<&String> {
    for (index, value) in values.iter().enumerate() {
        if values[..index].contains(value) {
            return Some(value);
        }
    }
    None
}

/// Normalizes and validates one allow path from the config. A leading `/` or
/// `./` is stripped (the paths are relative to the shared root); the result
/// is a normalized relative path where `""` is the root itself, covering
/// everything. Backslashes, NUL, `..` components and empty segments are
/// refused: the rules are written against the visible structure and a path
/// that cannot address it is a config error, not an access decision.
fn validate_allow_path(raw: &str) -> Result<String, String> {
    if raw.contains('\0') || raw.contains('\\') {
        return Err(format!(
            "Configuration access path \"{raw}\" is invalid; backslashes and NUL are not allowed."
        ));
    }
    let normalized = normalize_relative_path(raw);
    if normalized.is_empty() {
        // The root itself: `/` or `.` addresses the shared root, and a root
        // allow covers everything under it.
        return Ok(normalized);
    }
    for segment in normalized.split('/') {
        if segment.is_empty() {
            return Err(format!(
                "Configuration access path \"{raw}\" is invalid; segments must not be empty."
            ));
        }
        if segment == ".." {
            return Err(format!(
                "Configuration access path \"{raw}\" is invalid; `..` components are not allowed."
            ));
        }
    }
    Ok(normalized)
}

/// Normalizes and validates one deny pattern from the config. A leading `/`
/// or `./` is stripped (anchoring is the default for patterns with a
/// slash); a trailing `/` is refused (the gitignore directory-only marker is
/// not supported: a deny hides the named file or directory wherever it
/// appears), as are backslashes, NUL and `..` components.
fn validate_deny_pattern(raw: &str) -> Result<DenyPattern, String> {
    if raw.trim().is_empty() {
        return Err("Configuration access deny patterns must be non-empty strings.".to_string());
    }
    if raw.contains('\0') || raw.contains('\\') {
        return Err(format!(
            "Configuration access deny pattern \"{raw}\" is invalid; backslashes and NUL are not allowed."
        ));
    }
    if raw.ends_with('/') {
        return Err(format!(
            "Configuration access deny pattern \"{raw}\" is invalid; a trailing slash is not supported. Deny the directory by name instead."
        ));
    }
    let normalized = normalize_relative_path(raw);
    let segments: Vec<&str> = normalized.split('/').collect();
    for segment in &segments {
        if segment.is_empty() {
            return Err(format!(
                "Configuration access deny pattern \"{raw}\" is invalid; segments must not be empty."
            ));
        }
        if *segment == ".." {
            return Err(format!(
                "Configuration access deny pattern \"{raw}\" is invalid; `..` components are not allowed."
            ));
        }
    }

    let anchored = segments.len() > 1;
    let compiled = segments
        .iter()
        .map(|segment| SegmentMatcher {
            pattern: segment.to_string(),
            double_star: *segment == "**",
        })
        .collect();
    Ok(DenyPattern {
        raw: raw.to_string(),
        anchored,
        segments: compiled,
    })
}

/// Strips a leading `/` or `./` from a config path and collapses the result
/// to a normalized relative path (the config paths are relative to the shared
/// root, so `/common` and `common` are the same allow). Trailing slashes are
/// refused by the callers, so they never reach this normalization.
fn normalize_relative_path(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_leading_slash = trimmed.strip_prefix('/').unwrap_or(trimmed);
    let without_leading_dot_slash = without_leading_slash
        .strip_prefix("./")
        .unwrap_or(without_leading_slash);
    without_leading_dot_slash.to_string()
}

/// Whether one segment text matches one path segment: `*` matches anything
/// except `/` within the segment (including a leading dot), `?` matches one
/// character, everything else is literal. Case-sensitive.
fn segment_matches(pattern: &str, segment: &str) -> bool {
    glob_match_chars(pattern.as_bytes(), segment.as_bytes())
}

/// Backtracking glob match over one segment's bytes. `*` consumes any run of
/// bytes, `?` consumes exactly one byte, anything else must match literally.
fn glob_match_chars(pattern: &[u8], segment: &[u8]) -> bool {
    let (first, rest) = match pattern.split_first() {
        Some(split) => split,
        None => return segment.is_empty(),
    };
    match first {
        b'*' => {
            // The star consumes zero or more bytes; the backtracking loop
            // tries every split point so later pattern bytes can match.
            for index in 0..=segment.len() {
                if glob_match_chars(rest, &segment[index..]) {
                    return true;
                }
            }
            false
        }
        b'?' => {
            let (_, tail) = match segment.split_first() {
                Some(split) => split,
                None => return false,
            };
            glob_match_chars(rest, tail)
        }
        byte => {
            let (head, tail) = match segment.split_first() {
                Some(split) => split,
                None => return false,
            };
            head == byte && glob_match_chars(rest, tail)
        }
    }
}

impl AccessDenial {
    /// The report wording for this denial, shared by the verdict lines and
    /// the check report so the wording cannot drift.
    fn reason(&self) -> String {
        match self {
            AccessDenial::NoAllow => "no allow entry matches".to_string(),
            AccessDenial::Denied { pattern, source } => {
                format!("denied by \"{pattern}\" ({source})")
            }
        }
    }
}

impl DenyPattern {
    /// Whether the pattern denies the given normalized path segments. See the
    /// type's documentation for the matching rules.
    pub fn matches(&self, segments: &[&str]) -> bool {
        if !self.anchored {
            // Unanchored: a single-segment pattern matches any path segment.
            return self.segments.first().is_some_and(|matcher| {
                segments
                    .iter()
                    .any(|segment| segment_matches(&matcher.pattern, segment))
            });
        }
        match_from(&self.segments, segments, 0)
    }
}

/// Matches a compiled anchored pattern against path segments starting at
/// `offset`. A `**` middle segment matches zero or more path segments; a
/// `**` trailing segment matches one or more (gitignore: `a/**` matches
/// everything inside `a`, not `a` itself). When every pattern segment is
/// consumed the match succeeds: the path is the matched directory or under
/// it, so everything underneath is denied.
fn match_from(pattern: &[SegmentMatcher], segments: &[&str], offset: usize) -> bool {
    if pattern.is_empty() {
        return true;
    }
    if offset >= segments.len() {
        return false;
    }
    let matcher = &pattern[0];
    if matcher.double_star {
        let minimum_skip = if pattern.len() == 1 { 1 } else { 0 };
        let remaining = segments.len() - offset;
        if remaining < minimum_skip {
            return false;
        }
        for skip in minimum_skip..=remaining {
            if match_from(&pattern[1..], segments, offset + skip) {
                return true;
            }
        }
        false
    } else {
        segment_matches(&matcher.pattern, segments[offset])
            && match_from(&pattern[1..], segments, offset + 1)
    }
}

/// Whether an allow path covers the given normalized path segments: the
/// allow's segments are a case-sensitive prefix of the path's segments. The
/// root allow (`""`) covers everything. Allows take no wildcards.
fn allow_covers(allow_path: &str, segments: &[&str]) -> bool {
    if allow_path.is_empty() {
        return true;
    }
    let allow_segments: Vec<&str> = allow_path.split('/').collect();
    if allow_segments.len() > segments.len() {
        return false;
    }
    allow_segments
        .iter()
        .zip(segments)
        .all(|(allow_segment, path_segment)| allow_segment == path_segment)
}

/// The evaluation function: one place where the access semantics live
/// (ADR-0005). `segments` is the normalized relative path's segments (empty
/// for the root). The visible root is the union of the global allow list and
/// the allow entries of the user's configured groups, minus every matching
/// global deny and every allow-specific deny within its allow's scope: the
/// deny lists filter the unioned result, so a deny always wins within its
/// scope. No matching allow means the path is hidden (deny-by-default).
pub fn evaluate(access: &AccessConfig, groups: &[String], segments: &[&str]) -> AccessDecision {
    if segments.is_empty() {
        // The root itself: the visible root's existence is the verdict. A
        // user whose visible root is non-empty (the global allow list or any
        // of their groups' allow entries) can browse the root listing, and
        // its entries are filtered by the child evaluations below. A user
        // with no allow entries at all sees an empty visible root: the root
        // listing answers 404 and the SPA renders the no-access state. No
        // deny pattern can match the root (it has no segments to match
        // against).
        let has_allow = !access.global_allow.is_empty()
            || access
                .grants
                .iter()
                .any(|(group, grant)| groups.contains(group) && !grant.allow.is_empty());
        return if has_allow {
            AccessDecision::Visible {
                allow_source: "allow entries exist for the scenario's groups".to_string(),
            }
        } else {
            AccessDecision::Hidden(AccessDenial::NoAllow)
        };
    }

    // The global baseline and the allow entries of the user's configured
    // groups. Groups the config does not name are ignored. The global allow
    // entries never carry allow-specific denies, so the deny loop below only
    // iterates the group entries.
    let mut covered_by_global = false;
    let mut covering: Vec<(&str, &AllowEntry)> = Vec::new();
    for allow_path in &access.global_allow {
        if allow_covers(allow_path, segments) {
            covered_by_global = true;
        }
    }
    for (group, grant) in &access.grants {
        if !groups.contains(group) {
            continue;
        }
        for entry in &grant.allow {
            if allow_covers(&entry.path, segments) {
                covering.push((group.as_str(), entry));
            }
        }
    }

    if !covered_by_global && covering.is_empty() {
        return AccessDecision::Hidden(AccessDenial::NoAllow);
    }

    // The global deny applies to every user and every allow path.
    if let Some(pattern) = access.global_deny.iter().find(|p| p.matches(segments)) {
        return AccessDecision::Hidden(AccessDenial::Denied {
            pattern: pattern.raw.clone(),
            source: "global deny".to_string(),
        });
    }

    // Every allow-specific deny whose allow covers the path filters the
    // unioned result: a deny always wins within its scope, regardless of
    // which covering allow made the path visible.
    for (group, entry) in &covering {
        if let Some(pattern) = entry.deny.iter().find(|p| p.matches(segments)) {
            return AccessDecision::Hidden(AccessDenial::Denied {
                pattern: pattern.raw.clone(),
                source: format!(
                    "allow-specific deny under \"/{path}\" (grant {group})",
                    path = entry.path,
                    group = group
                ),
            });
        }
    }

    // Visible. The report names the first covering allow; every covering
    // allow would make the path visible, so naming one is enough to answer
    // "why is this visible".
    let allow_source = if covered_by_global {
        "global allow".to_string()
    } else {
        let (group, entry) = covering[0];
        format!(
            "allow \"/{path}\" (grant {group})",
            path = entry.path,
            group = group
        )
    };
    AccessDecision::Visible { allow_source }
}

/// The delete authorization check (ADR-0006): a user may delete a file if
/// and only if the file is visible to them under the existing allow/deny
/// evaluation, and an allow entry of one of their groups that has
/// `delete: true` covers the file. Deny rules still win: a file hidden by
/// the global deny or by any allow-specific deny is not deletable even when
/// a delete-enabled grant's allow covers it. Files visible only through the
/// global allow baseline are never deletable, since only grant allow
/// entries count. The root itself is not a file.
pub fn can_delete(access: &AccessConfig, groups: &[String], relative: &str) -> bool {
    if relative.is_empty() || !is_path_visible(access, groups, relative) {
        return false;
    }
    let segments = path_segments(relative);
    access.grants.iter().any(|(group, grant)| {
        groups.contains(group)
            && grant.delete
            && grant
                .allow
                .iter()
                .any(|entry| allow_covers(&entry.path, &segments))
    })
}

/// The normalized relative path segments of a validated relative path.
fn path_segments(relative: &str) -> Vec<&str> {
    if relative.is_empty() {
        Vec::new()
    } else {
        relative.split('/').collect()
    }
}

/// The fully-decoded form of an API-rendered path: the form the handlers
/// open through the percent-decode fallback when the literal form does not
/// exist. Access rules must hold for the decoded form too, or a request that
/// only resolves after a second decode would bypass the deny rules (the gate
/// evaluates the once-decoded request path; the handler opens the decoded
/// path). Returns None when the decode is invalid (a component decoding to
/// `/`, `\\`, NUL or `..`) or when the decoded bytes are not valid UTF-8:
/// the handlers reject such requests and the decoded form cannot be
/// referenced by deny patterns, so the literal evaluation decides.
fn decode_equivalent(relative: &str) -> Option<String> {
    let mut decoded = String::new();
    let mut first = true;
    for component in relative.split('/') {
        if !first {
            decoded.push('/');
        }
        first = false;
        let bytes = crate::percent_decode_bytes(component);
        if bytes.contains(&b'/') || bytes.contains(&b'\\') || bytes.contains(&0) || bytes == b".." {
            return None;
        }
        match std::str::from_utf8(&bytes) {
            // Empty components carry no path information and decode away.
            Ok("") => {}
            Ok(text) => decoded.push_str(text),
            Err(_) => return None,
        }
    }
    Some(decoded)
}

/// The runtime visibility check for a validated relative path: the access
/// decision the handlers and the access gate middleware act on. Deny wins
/// over the decode equivalence: when the fully-decoded form differs, it must
/// also be visible, or the percent-decode fallback would serve a denied file
/// for a request whose literal form matches no deny. An invalid decode means
/// the handlers reject the decoded form anyway, so the literal evaluation
/// decides.
pub fn is_path_visible(access: &AccessConfig, groups: &[String], relative: &str) -> bool {
    if is_hidden(access, groups, relative) {
        return false;
    }
    match decode_equivalent(relative) {
        Some(decoded) if decoded != relative => !is_hidden(access, groups, &decoded),
        _ => true,
    }
}

/// The hidden verdict for one relative path.
fn is_hidden(access: &AccessConfig, groups: &[String], relative: &str) -> bool {
    matches!(
        evaluate(access, groups, &path_segments(relative)),
        AccessDecision::Hidden(_)
    )
}

/// One line of the `--check-access` report for a directory: the verdict for
/// the scenario's groups with the rule that caused it.
fn verdict_line(relative: &str, decision: &AccessDecision) -> String {
    let path_display = if relative.is_empty() {
        "/".to_string()
    } else {
        format!("/{relative}")
    };
    match decision {
        AccessDecision::Visible { allow_source } => {
            format!("{path_display}  visible ({allow_source})")
        }
        AccessDecision::Hidden(denial) => {
            format!("{path_display}  hidden: {}", denial.reason())
        }
    }
}

/// Builds the `--check-access` report: the server starts with the real
/// production config, walks the real shared root for the scenario's groups
/// and prints the verdicts with rule provenance, then the caller exits. The
/// walk covers every directory reachable under the visible root; a hidden
/// directory is reported once (everything underneath is blocked by the same
/// verdict) and not walked. Within a visible directory the blocked entries
/// are listed: a blocked folder is marked with everything underneath blocked,
/// a blocked file individually, so mismatches between the rules and the
/// actual structure are easy to see and sensitive files are verifiable.
/// Rules that match nothing (an allow naming a path the walk never evaluates,
/// or a deny pattern no evaluated path hits) are reported plainly at the end
/// (ADR-0005 decision 9), so a stale rule cannot hide behind silence.
pub async fn run_access_check(
    shared_root: &Path,
    access: &AccessConfig,
    groups: &[String],
) -> Result<String, String> {
    let mut report = String::new();
    report.push_str("Access check\n");
    report.push_str(&format!("groups: {}\n", groups.join(", ")));
    report.push_str(&format!("shared root: {}\n", shared_root.display()));
    report.push('\n');

    let mut rules = check_rules(access);
    let mut queue: Vec<String> = vec![String::new()];
    while let Some(relative) = queue.pop() {
        let segments = path_segments(&relative);
        let decision = evaluate(access, groups, &segments);
        // Rule usage is tracked against every evaluated path: an allow rule
        // is used when its coverage covers one, a deny rule when its pattern
        // matches one.
        for rule in rules.iter_mut() {
            if let Some(allow_path) = &rule.allow_path
                && allow_covers(allow_path, &segments)
            {
                rule.used = true;
            }
            if let Some(pattern) = &rule.deny
                && pattern.matches(&segments)
            {
                rule.used = true;
            }
        }
        report.push_str(&verdict_line(&relative, &decision));
        report.push('\n');

        if matches!(decision, AccessDecision::Hidden(_)) {
            // Everything underneath is blocked by the same verdict.
            continue;
        }

        let directory = shared_root.join(&relative);
        let mut reader = tokio::fs::read_dir(&directory)
            .await
            .map_err(|_| format!("The shared directory \"{}\" could not be read; the access check needs the real structure.", directory.display()))?;
        let mut blocked: Vec<(String, String)> = Vec::new();
        let mut subdirs: Vec<String> = Vec::new();
        while let Some(item) = reader.next_entry().await.map_err(|_| {
            format!(
                "The shared directory \"{}\" could not be read.",
                directory.display()
            )
        })? {
            let raw_name = item.file_name();
            let name = if let Some(name) = raw_name.to_str() {
                name.to_string()
            } else {
                // A name that is not valid UTF-8 cannot be written into the
                // report; the check never addresses it through the API.
                continue;
            };
            let child_segments: Vec<&str> = segments
                .iter()
                .copied()
                .chain(std::iter::once(name.as_str()))
                .collect();
            let child_decision = evaluate(access, groups, &child_segments);
            // Child paths are evaluated too, so their rule usage counts.
            for rule in rules.iter_mut() {
                if let Some(allow_path) = &rule.allow_path
                    && allow_covers(allow_path, &child_segments)
                {
                    rule.used = true;
                }
                if let Some(pattern) = &rule.deny
                    && pattern.matches(&child_segments)
                {
                    rule.used = true;
                }
            }
            let is_dir = item.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            match &child_decision {
                AccessDecision::Visible { .. } => {
                    if is_dir {
                        subdirs.push(if relative.is_empty() {
                            name.clone()
                        } else {
                            format!("{relative}/{name}")
                        });
                    }
                    continue;
                }
                AccessDecision::Hidden(denial) => {
                    let kind = if is_dir {
                        "directory (everything underneath is blocked)"
                    } else {
                        "file"
                    };
                    blocked.push((format!("{name} ({kind}): {}", denial.reason()), name));
                }
            }
        }

        // Deterministic order: the blocked lists and the walk queue are
        // sorted so the report is stable regardless of readdir order. The
        // queue is a LIFO stack, so the subdirectories are pushed in
        // descending order and popped in ascending order.
        blocked.sort_by(|left, right| left.1.cmp(&right.1));
        if blocked.is_empty() {
            report.push_str("  blocked entries: none\n");
        } else {
            report.push_str("  blocked entries:\n");
            for (line, _) in blocked {
                report.push_str(&format!("    {line}\n"));
            }
        }
        report.push('\n');
        subdirs.sort();
        subdirs.reverse();
        queue.extend(subdirs);
    }

    // Rules that matched nothing (ADR-0005 decision 9): reported plainly, no
    // similarity hints. An allow entry naming a path the walk never evaluated
    // and a deny pattern no evaluated path hits are both stale for this
    // scenario.
    let mut unmatched: Vec<String> = rules
        .iter()
        .filter(|rule| !rule.used)
        .map(|rule| rule.label.clone())
        .collect();
    unmatched.sort();
    if unmatched.is_empty() {
        report.push_str("rules that matched nothing: none\n");
    } else {
        report.push_str("rules that matched nothing:\n");
        for label in unmatched {
            report.push_str(&format!("  {label}\n"));
        }
    }

    Ok(report)
}

/// One rule of the scenario's config with its operator-facing label, tracked
/// during the check walk so rules that match nothing are reported. The label
/// is the provenance form the report prints; the allow path and the deny
/// pattern carry the matching the walk checks against.
struct CheckRule {
    label: String,
    /// The plain allow path whose segment-prefix coverage is tracked; None
    /// for deny rules.
    allow_path: Option<String>,
    /// The deny pattern whose match is tracked; None for allow rules.
    deny: Option<DenyPattern>,
    used: bool,
}

/// The scenario's rules as check-tracked entries: the global allow list and
/// deny list, and each grant's allow entries with their paired allow-specific
/// denies.
fn check_rules(access: &AccessConfig) -> Vec<CheckRule> {
    let mut rules = Vec::new();
    for path in &access.global_allow {
        rules.push(CheckRule {
            label: format!("allow \"/{path}\" (global allow)"),
            allow_path: Some(path.clone()),
            deny: None,
            used: false,
        });
    }
    for pattern in &access.global_deny {
        rules.push(CheckRule {
            label: format!("denied by \"{}\" (global deny)", pattern.raw),
            allow_path: None,
            deny: Some(pattern.clone()),
            used: false,
        });
    }
    for (group, grant) in &access.grants {
        for entry in &grant.allow {
            rules.push(CheckRule {
                label: format!("allow \"/{}\" (grant {group})", entry.path),
                allow_path: Some(entry.path.clone()),
                deny: None,
                used: false,
            });
            for pattern in &entry.deny {
                rules.push(CheckRule {
                    label: format!(
                        "denied by \"{}\" (allow-specific deny under \"/{}\" (grant {group}))",
                        pattern.raw, entry.path
                    ),
                    allow_path: None,
                    deny: Some(pattern.clone()),
                    used: false,
                });
            }
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// A config covering the example shape: global baseline `/common`, global
    /// deny for `.env` and `.git`, a tv-shows grant with an allow-specific
    /// `.nfo` deny, and a devs grant with two allow entries.
    fn example_config() -> AccessConfig {
        AccessConfig {
            global_allow: vec!["common".to_string()],
            global_deny: deny_patterns(&[".env", ".git"]),
            grants: BTreeMap::from([
                (
                    "yafm-tv".to_string(),
                    Grant {
                        delete: false,
                        allow: vec![AllowEntry {
                            path: "tv-shows".to_string(),
                            deny: deny_patterns(&["*.nfo"]),
                        }],
                    },
                ),
                (
                    "yafm-devs".to_string(),
                    Grant {
                        delete: false,
                        allow: vec![
                            AllowEntry {
                                path: "dev".to_string(),
                                deny: deny_patterns(&["dev/tmp/**"]),
                            },
                            AllowEntry {
                                path: "projects".to_string(),
                                deny: Vec::new(),
                            },
                        ],
                    },
                ),
            ]),
        }
    }

    fn deny_patterns(raw: &[&str]) -> Vec<DenyPattern> {
        raw.iter()
            .map(|pattern| validate_deny_pattern(pattern).expect("a valid test pattern"))
            .collect()
    }

    fn visible(config: &AccessConfig, groups: &[&str], relative: &str) -> bool {
        let groups: Vec<String> = groups.iter().map(|g| g.to_string()).collect();
        is_path_visible(config, &groups, relative)
    }

    /// A config with a delete-enabled grant scoped to `/dev` and a grant
    /// without delete scoped to `/tv-shows`.
    fn delete_config() -> AccessConfig {
        AccessConfig {
            global_allow: vec!["common".to_string()],
            global_deny: deny_patterns(&[".env"]),
            grants: BTreeMap::from([
                (
                    "yafm-devs".to_string(),
                    Grant {
                        delete: true,
                        allow: vec![AllowEntry {
                            path: "dev".to_string(),
                            deny: Vec::new(),
                        }],
                    },
                ),
                (
                    "yafm-tv".to_string(),
                    Grant {
                        delete: false,
                        allow: vec![AllowEntry {
                            path: "tv-shows".to_string(),
                            deny: Vec::new(),
                        }],
                    },
                ),
            ]),
        }
    }

    #[test]
    fn can_delete_requires_a_delete_enabled_grant_whose_allow_covers_the_path() {
        let config = delete_config();
        let groups: Vec<String> = vec!["yafm-devs".to_string(), "yafm-tv".to_string()];

        // A delete-enabled grant's allow covers the path: deletable.
        assert!(can_delete(&config, &groups, "dev/main.rs"));
        // A visible file covered only by a grant without delete: not deletable.
        assert!(!can_delete(&config, &groups, "tv-shows/show.mkv"));
        // The global baseline is never deletable: only grant allow entries count.
        assert!(!can_delete(&config, &groups, "common/README.md"));
    }

    #[test]
    fn can_delete_follows_the_deny_rules_and_refuses_the_root() {
        let config = AccessConfig {
            global_allow: vec![],
            global_deny: deny_patterns(&[".env"]),
            grants: BTreeMap::from([(
                "yafm-devs".to_string(),
                Grant {
                    delete: true,
                    allow: vec![AllowEntry {
                        path: "dev".to_string(),
                        deny: deny_patterns(&["dev/tmp/**"]),
                    }],
                },
            )]),
        };
        let groups: Vec<String> = vec!["yafm-devs".to_string()];

        assert!(can_delete(&config, &groups, "dev/main.rs"));
        // A deny always wins: the global deny hides .env and the
        // allow-specific deny hides dev/tmp, so neither is deletable even
        // though a delete-enabled grant's allow covers both.
        assert!(!can_delete(&config, &groups, "dev/.env"));
        assert!(!can_delete(&config, &groups, "dev/tmp/scratch.txt"));
        // The root itself is not a file.
        assert!(!can_delete(&config, &groups, ""));
        // A group the config does not name carries no delete for the user.
        assert!(!can_delete(
            &config,
            &["yafm-other".to_string()],
            "dev/main.rs"
        ));
    }

    #[test]
    fn initialize_access_requires_the_access_block() {
        // The OnceCell global is uninitialized in this test binary: a missing
        // `access` block must abort startup, not start the server with no
        // access configuration (ADR-0005, fail closed).
        let error = initialize_access().expect_err("a missing access block aborts startup");
        assert_eq!(
            error,
            "Configuration access is required; the backend refuses to start without an access configuration."
        );
    }

    #[test]
    fn the_decode_equivalent_form_cannot_bypass_the_deny_rules() {
        let config = example_config();
        let tv = ["yafm-tv"];

        // The percent-encoded forms the handlers open through the
        // percent-decode fallback: the literal evaluation alone sees no deny
        // match, but the fully-decoded form is a denied file. Deny wins over
        // the decode class, so the encoded attempt is not visible either.
        assert!(decode_equivalent("show.s01e01%2Enfo") == Some("show.s01e01.nfo".to_string()));
        assert!(!visible(&config, &tv, "tv-shows/season-1/show.s01e01.nfo"));
        assert!(!visible(
            &config,
            &tv,
            "tv-shows/season-1/show.s01e01%2Enfo"
        ));

        // The same for an anchored allow-specific deny: dev/%74mp/x only
        // resolves to dev/tmp/x after the second decode, and the encoded
        // attempt is not visible.
        let devs = ["yafm-devs"];
        assert!(!visible(&config, &devs, "dev/tmp/x"));
        assert!(!visible(&config, &devs, "dev/%74mp/x"));

        // Allowed encoded names stay resolvable: the decoded form is visible.
        assert!(visible(&config, &devs, "dev/main%2Ers"));

        // Invalid decodes are rejected by the handlers anyway, so the literal
        // evaluation decides.
        assert!(decode_equivalent("a%252Fb") == Some("a%2Fb".to_string()));
        assert!(decode_equivalent("a%2Fb").is_none());
        assert!(decode_equivalent("%2E%2E").is_none());
        assert!(decode_equivalent("a%2E%2Eb") == Some("a..b".to_string()));
    }

    #[test]
    fn the_example_shape_evaluates_as_expected() {
        let config = example_config();

        // The global baseline: every user sees /common.
        assert!(visible(&config, &[], "common"));
        assert!(visible(&config, &["yafm-tv"], "common/README.md"));
        assert!(visible(&config, &["yafm-devs"], "common/sub"));

        // Deny-by-default: nothing else for a user with no matching grants.
        assert!(!visible(&config, &[], "tv-shows"));
        assert!(!visible(&config, &[], "dev"));

        // The tv-shows grant: visible, but .nfo files denied anywhere under
        // it by the allow-specific deny.
        assert!(visible(&config, &["yafm-tv"], "tv-shows"));
        assert!(visible(&config, &["yafm-tv"], "tv-shows/season-1"));
        assert!(!visible(
            &config,
            &["yafm-tv"],
            "tv-shows/season-1/show.s01e01.nfo"
        ));
        assert!(visible(
            &config,
            &["yafm-tv"],
            "tv-shows/season-1/show.s01e01.mkv"
        ));

        // The devs grant: two allow entries, with tmp/** denied under /dev.
        assert!(visible(&config, &["yafm-devs"], "projects"));
        assert!(visible(&config, &["yafm-devs"], "dev/src/main.rs"));
        assert!(!visible(&config, &["yafm-devs"], "dev/tmp/scratch.txt"));
        // gitignore semantics: the wildcard hides everything inside dev/tmp,
        // not dev/tmp itself; hiding the folder entirely means denying it by
        // name or with the bare name.
        assert!(visible(&config, &["yafm-devs"], "dev/tmp"));

        // The global deny applies to every user and every allow path.
        assert!(!visible(&config, &["yafm-devs"], "dev/.env"));
        assert!(!visible(&config, &[], "common/.git"));
        assert!(!visible(&config, &["yafm-tv"], "tv-shows/.git"));
    }

    #[test]
    fn groups_the_config_does_not_name_are_ignored() {
        let config = example_config();
        // A user in an unconfigured group sees only the global baseline.
        assert!(visible(&config, &["yafm-other"], "common"));
        assert!(!visible(&config, &["yafm-other"], "tv-shows"));
        // Unconfigured groups alongside configured ones contribute nothing.
        assert!(visible(&config, &["yafm-tv", "yafm-other"], "tv-shows"));
    }

    #[test]
    fn denies_win_over_allows_within_their_scope() {
        // A path covered by two allows: one with a matching allow-specific
        // deny, one without. The deny lists filter the unioned result, so the
        // deny wins regardless of which allow is evaluated first.
        let config = AccessConfig {
            global_allow: Vec::new(),
            global_deny: Vec::new(),
            grants: BTreeMap::from([
                (
                    "a".to_string(),
                    Grant {
                        delete: false,
                        allow: vec![AllowEntry {
                            path: "shared".to_string(),
                            deny: deny_patterns(&["*.nfo"]),
                        }],
                    },
                ),
                (
                    "b".to_string(),
                    Grant {
                        delete: false,
                        allow: vec![AllowEntry {
                            path: "shared".to_string(),
                            deny: Vec::new(),
                        }],
                    },
                ),
            ]),
        };
        assert!(!visible(&config, &["a", "b"], "shared/show.nfo"));
        assert!(visible(&config, &["a", "b"], "shared/show.mkv"));
        // A user only in group a keeps the deny.
        assert!(!visible(&config, &["a"], "shared/show.nfo"));
    }

    #[test]
    fn the_root_allow_covers_everything_under_it() {
        let config = AccessConfig {
            global_allow: vec![validate_allow_path("/").expect("the root allow")],
            global_deny: Vec::new(),
            grants: BTreeMap::new(),
        };
        assert!(visible(&config, &[], ""));
        assert!(visible(&config, &[], "anything/deep/file.txt"));
    }

    #[test]
    fn anchored_patterns_match_a_prefix_starting_at_the_first_segment() {
        let pattern = validate_deny_pattern("foo/bar").expect("a valid pattern");
        let matches = |relative: &str| pattern.matches(&path_segments(relative));
        assert!(matches("foo/bar"));
        assert!(matches("foo/bar/baz"));
        assert!(!matches("foo"));
        assert!(!matches("x/foo/bar"));
        assert!(!matches("foobar"));

        // A segment glob does not cross a slash.
        let pattern = validate_deny_pattern("tv-shows/*.nfo").expect("a valid pattern");
        let matches = |relative: &str| pattern.matches(&path_segments(relative));
        assert!(matches("tv-shows/x.nfo"));
        assert!(!matches("tv-shows/sub/x.nfo"));
    }

    #[test]
    fn unanchored_patterns_match_a_segment_at_any_depth() {
        let pattern = validate_deny_pattern("node_modules").expect("a valid pattern");
        let matches = |relative: &str| pattern.matches(&path_segments(relative));
        assert!(matches("node_modules"));
        assert!(matches("a/node_modules/x"));
        assert!(!matches("node_modules_backup"));

        let pattern = validate_deny_pattern("*.nfo").expect("a valid pattern");
        let matches = |relative: &str| pattern.matches(&path_segments(relative));
        assert!(matches("tv-shows/x.nfo"));
        assert!(matches("x.nfo"));
        assert!(!matches("tv-shows/x.mkv"));
    }

    #[test]
    fn double_star_matches_gitignore_semantics() {
        // A middle ** matches zero or more segments.
        let pattern = validate_deny_pattern("a/**/b").expect("a valid pattern");
        let matches = |relative: &str| pattern.matches(&path_segments(relative));
        assert!(matches("a/b"));
        assert!(matches("a/x/b"));
        assert!(matches("a/x/y/b"));

        // A trailing ** matches one or more segments: a/** hides everything
        // inside a, not a itself.
        let pattern = validate_deny_pattern("tmp/**").expect("a valid pattern");
        let matches = |relative: &str| pattern.matches(&path_segments(relative));
        assert!(matches("tmp/x"));
        assert!(matches("tmp/x/y"));
        assert!(!matches("tmp"));

        // A leading ** matches at any depth.
        let pattern = validate_deny_pattern("**/node_modules").expect("a valid pattern");
        let matches = |relative: &str| pattern.matches(&path_segments(relative));
        assert!(matches("node_modules"));
        assert!(matches("a/node_modules"));
        assert!(matches("a/node_modules/x"));
        assert!(!matches("node_modulesx"));
    }

    #[test]
    fn glob_matching_supports_question_marks_and_is_case_sensitive() {
        let pattern = validate_deny_pattern("file?.txt").expect("a valid pattern");
        let matches = |relative: &str| pattern.matches(&path_segments(relative));
        assert!(matches("file1.txt"));
        assert!(matches("sub/fileA.txt"));
        assert!(!matches("file10.txt"));

        let pattern = validate_deny_pattern(".ENV").expect("a valid pattern");
        let matches = |relative: &str| pattern.matches(&path_segments(relative));
        // Case-sensitive: a deny pattern only matches the exact case, so a
        // .env file is not denied by a .ENV pattern.
        assert!(!matches(".env"));
        assert!(matches(".ENV"));
    }

    #[test]
    fn validation_refuses_invalid_allow_paths_and_deny_patterns() {
        let error = validate_allow_path("../escape").expect_err("`..` must be refused");
        assert!(error.contains("`..` components are not allowed"));

        let error =
            validate_allow_path("with\\backslash").expect_err("a backslash must be refused");
        assert!(error.contains("backslash"));

        let error = validate_allow_path("/foo//bar").expect_err("empty segments must be refused");
        assert!(error.contains("segments must not be empty"));

        let error = validate_deny_pattern("tmp/").expect_err("a trailing slash must be refused");
        assert!(error.contains("trailing slash is not supported"));

        let error = validate_deny_pattern("").expect_err("an empty pattern must be refused");
        assert!(error.contains("non-empty"));

        let error = validate_deny_pattern("a/../b").expect_err("`..` segments must be refused");
        assert!(error.contains("`..`"));
    }

    #[test]
    fn leading_slash_and_dot_slash_normalize_to_the_same_allow() {
        let slashed = validate_allow_path("/common").expect("a valid path");
        let plain = validate_allow_path("common").expect("a valid path");
        let dotted = validate_allow_path("./common").expect("a valid path");
        assert_eq!(slashed, "common");
        assert_eq!(slashed, plain);
        assert_eq!(slashed, dotted);
    }

    #[test]
    fn validation_refuses_duplicate_entries_and_empty_grants() {
        let block = AccessConfigFile {
            allow: vec!["common".to_string(), "common".to_string()],
            deny: Vec::new(),
            grants: BTreeMap::new(),
        };
        let error =
            validate_access_config(block).expect_err("duplicate allow entries must be refused");
        assert!(error.contains("more than once"));

        let block = AccessConfigFile {
            allow: Vec::new(),
            deny: Vec::new(),
            grants: BTreeMap::from([(
                "yafm-tv".to_string(),
                GrantFile {
                    delete: None,
                    allow: Vec::new(),
                },
            )]),
        };
        let error = validate_access_config(block).expect_err("an empty grant must be refused");
        assert!(error.contains("at least one allow entry"));
    }

    /// The check report against a real temp tree: allowed directories stated
    /// once, denied folders marked with everything underneath blocked, denied
    /// files listed individually, and hidden directories reported once.
    #[tokio::test]
    async fn the_access_check_report_matches_the_actual_structure() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root = temp.path().join("share");
        let tv = root.join("tv-shows");
        let season = tv.join("season-1");
        let dev = root.join("dev");
        let dev_tmp = dev.join("tmp");
        let common = root.join("common");
        for directory in [&tv, &season, &dev, &dev_tmp, &common] {
            fs::create_dir_all(directory).expect("create the tree");
        }
        fs::write(season.join("show.s01e01.nfo"), "metadata").expect("write nfo");
        fs::write(season.join("show.s01e01.mkv"), "video").expect("write mkv");
        fs::write(dev.join(".env"), "secret").expect("write env");
        fs::write(dev.join("main.rs"), "code").expect("write code");

        let config = example_config();
        let report = run_access_check(&root, &config, &["yafm-tv".to_string()])
            .await
            .expect("the check report");

        assert!(report.contains("groups: yafm-tv"), "{report}");
        // /common visible via the global allow.
        assert!(
            report.contains("/common  visible (global allow)"),
            "{report}"
        );
        // /tv-shows visible via the grant; the .nfo file is blocked
        // individually, the .mkv is not.
        assert!(
            report.contains("/tv-shows  visible (allow \"/tv-shows\" (grant yafm-tv))"),
            "{report}"
        );
        assert!(
            report.contains(
                "show.s01e01.nfo (file): denied by \"*.nfo\" (allow-specific deny under \"/tv-shows\" (grant yafm-tv))"
            ),
            "{report}"
        );
        // Visible entries are summarized by the directory verdict (only the
        // blocked entries are listed), so the visible .mkv does not appear.
        assert!(!report.contains("show.s01e01.mkv"), "{report}");
        // /dev is hidden for yafm-tv: no allow entry matches. The root
        // listing is visible for the scenario, so /dev appears as a blocked
        // entry under it (everything underneath blocked) and is not walked.
        assert!(
            report.contains(
                "dev (directory (everything underneath is blocked)): no allow entry matches"
            ),
            "{report}"
        );
        // The hidden /dev is not walked: its .env and main.rs never appear
        // as blocked entries (the unused-rule section can name the global
        // .env deny, which matches nothing in this scenario).
        assert!(!report.contains(".env (file)"), "{report}");
        assert!(!report.contains("main.rs"), "{report}");
        // season-1 is visible and walked, with the .nfo blocked.
        assert!(report.contains("/tv-shows/season-1  visible"), "{report}");

        // Rules that matched nothing in the scenario, reported plainly: the
        // nonexistent /projects (the yafm-devs allow covers it but the walk
        // never evaluates it), the yafm-devs deny with no matching entry, and
        // the global denies that no evaluated path hits. The fired *.nfo deny
        // is not among them, and neither is allow /dev: it covers the real
        // blocked dev entry, so it is not stale.
        assert!(report.contains("rules that matched nothing:"), "{report}");
        assert!(
            report.contains("allow \"/projects\" (grant yafm-devs)"),
            "{report}"
        );
        assert!(
            !report.contains("allow \"/dev\" (grant yafm-devs)"),
            "{report}"
        );
        assert!(
            report.contains("denied by \".git\" (global deny)"),
            "{report}"
        );
        assert!(
            !report.contains("rules that matched nothing:\n    denied by \"*.nfo\""),
            "{report}"
        );

        // A scenario with no allow entries at all (no global baseline and no
        // matching grants): the root is hidden and nothing is walked.
        let empty_config = AccessConfig {
            global_allow: Vec::new(),
            global_deny: Vec::new(),
            grants: BTreeMap::new(),
        };
        let report = run_access_check(&root, &empty_config, &[])
            .await
            .expect("the check report for an empty scenario");
        assert!(
            report.contains("/  hidden: no allow entry matches"),
            "{report}"
        );
        // No rules configured: nothing to report as unused.
        assert!(
            report.contains("rules that matched nothing: none"),
            "{report}"
        );
    }

    /// A rule naming a path the walk never evaluates is reported: an allow
    /// for a nonexistent directory and a deny pattern with no matching entry
    /// (ADR-0005 decision 9, no similarity hints).
    #[tokio::test]
    async fn the_access_check_reports_rules_that_match_nothing() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root = temp.path().join("share");
        fs::create_dir_all(root.join("common")).expect("create the tree");

        let config = AccessConfig {
            global_allow: vec![validate_allow_path("/common").expect("the root allow")],
            global_deny: deny_patterns(&[".env", "stale/**"]),
            grants: BTreeMap::from([(
                "yafm-devs".to_string(),
                Grant {
                    delete: false,
                    allow: vec![AllowEntry {
                        path: validate_allow_path("/missing").expect("the allow path"),
                        deny: Vec::new(),
                    }],
                },
            )]),
        };
        let report = run_access_check(&root, &config, &["yafm-devs".to_string()])
            .await
            .expect("the check report");

        assert!(report.contains("rules that matched nothing:"), "{report}");
        // The allow names a directory that does not exist on disk.
        assert!(
            report.contains("allow \"/missing\" (grant yafm-devs)"),
            "{report}"
        );
        // The deny patterns match no evaluated path in this scenario.
        assert!(
            report.contains("denied by \".env\" (global deny)"),
            "{report}"
        );
        assert!(
            report.contains("denied by \"stale/**\" (global deny)"),
            "{report}"
        );
    }

    #[tokio::test]
    async fn the_access_check_walks_visible_subdirectories_of_the_root_allow() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let root = temp.path().join("share");
        fs::create_dir_all(root.join("everything/deep")).expect("create the tree");
        fs::write(root.join("everything/deep/file.txt"), "data").expect("write file");
        fs::write(root.join("everything/.secret"), "sensitive").expect("write secret");

        let config = AccessConfig {
            global_allow: vec![validate_allow_path("/").expect("the root allow")],
            global_deny: deny_patterns(&[".secret"]),
            grants: BTreeMap::new(),
        };
        let report = run_access_check(&root, &config, &[])
            .await
            .expect("the check report");

        assert!(report.contains("/everything  visible"), "{report}");
        assert!(report.contains("/everything/deep  visible"), "{report}");
        assert!(
            report.contains(".secret (file): denied by \".secret\" (global deny)"),
            "{report}"
        );
    }

    #[test]
    fn the_verdict_line_reports_hidden_paths_with_the_rule_that_caused_it() {
        let config = example_config();
        let decision = evaluate(&config, &["yafm-tv".to_string()], &path_segments("dev"));
        let line = verdict_line("dev", &decision);
        assert!(line.contains("hidden: no allow entry matches"), "{line}");

        let decision = evaluate(
            &config,
            &["yafm-tv".to_string()],
            &path_segments("tv-shows/x.nfo"),
        );
        let line = verdict_line("tv-shows/x.nfo", &decision);
        assert!(line.contains("denied by \"*.nfo\""), "{line}");
        assert!(
            line.contains("allow-specific deny under \"/tv-shows\""),
            "{line}"
        );

        let decision = evaluate(&config, &Vec::<String>::new(), &path_segments("dev/.env"));
        let line = verdict_line("dev/.env", &decision);
        // The allow check comes first: /dev is hidden for a user with no
        // matching grants by deny-by-default, so the verdict is the same as
        // its parent's even though the global deny also matches .env.
        assert!(line.contains("hidden: no allow entry matches"), "{line}");
    }
}
