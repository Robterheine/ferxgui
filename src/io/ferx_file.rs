/// Parser for `.ferx` model files.
///
/// Extracts parameter names, initial values, and bounds from `[parameters]`
/// and `[initial_values]` sections.  Does not attempt to evaluate the
/// structural model — only what is needed to populate the GUI (model list
/// description, parameters pill, editor syntax highlighting tokens).
///
/// .ferx DSL quick reference:
///   [parameters]
///     theta TVCL(0.134, 0.001, 10.0)   # name(init, lower, upper)
///     omega ETA_CL ~ 0.07               # name ~ variance
///     sigma PROP_ERR ~ 0.01
///     theta TVCL(0.134, 0.001, 10.0) prior(0.15, rse = 10%)   # inline prior (MAP)
///
/// `#` and `//` both start a comment. `[initial_values]` is no longer valid
/// (ferx rejects it with E_DEPRECATED_BLOCK); the section name is still
/// recognised so section tracking stays correct, but it is not applied.
use crate::domain::{DeclaredPrior, ParsedParams};

// ---------------------------------------------------------------------------
// Public
// ---------------------------------------------------------------------------

/// Parse the full source text of a `.ferx` file.  Never panics; returns
/// whatever could be extracted.
pub fn parse_params(source: &str) -> ParsedParams {
    let mut p = ParsedParams::default();

    // Walk sections.
    let mut current_section: Option<&str> = None;
    for line in source.lines() {
        let trimmed = line.trim();
        if is_comment_line(trimmed) || trimmed.is_empty() {
            // First non-empty comment becomes the description.
            if p.description.is_empty() && is_comment_line(trimmed) {
                let desc = trimmed.trim_start_matches(['#', '/']).trim();
                if !desc.is_empty() {
                    p.description = desc.to_owned();
                }
            }
            continue;
        }
        if is_section_header(trimmed) {
            // Any bracketed header ends the previous section, whether or not
            // it's one we recognise — an unrecognised section (e.g. a DSL
            // block newer than this parser) must not leave `current_section`
            // pointing at whatever came before it, or that section's content
            // lines would keep being misattributed to it.
            current_section = section_name(trimmed);
            continue;
        }
        match current_section {
            Some("parameters") => parse_parameter_line(trimmed, &mut p),
            Some("priors") => parse_priors_line(trimmed, &mut p),
            _ => {}
        }
    }

    p
}

// ---------------------------------------------------------------------------
// Fit options
// ---------------------------------------------------------------------------

/// Values extracted from the `[fit_options]` block. A field is `None` when the
/// directive is absent or commented out — the model file is the source of truth,
/// so the GUI run controls are initialised from these when a model is loaded.
#[derive(Debug, Clone, Default)]
pub struct FitOptions {
    pub method: Option<String>,
    pub covariance: Option<bool>,
    pub gradient: Option<String>,
    pub threads: Option<u32>,
}

/// Parse the `[fit_options]` block of a `.ferx` file. Full-line and inline `#`
/// comments are ignored, so a commented-out directive reads as absent (`None`).
pub fn parse_fit_options(source: &str) -> FitOptions {
    let mut opts = FitOptions::default();
    let mut in_section = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if is_comment_line(trimmed) || trimmed.is_empty() {
            continue;
        }
        if is_section_header(trimmed) {
            in_section = section_name(trimmed) == Some("fit_options");
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, val)) = trimmed.split_once('=') else { continue; };
        // Strip any inline comment from the value.
        let val = strip_inline_comment(val).trim();
        match key.trim() {
            "method"   if !val.is_empty() => opts.method = Some(normalise_method_chain(val)),
            "gradient" if !val.is_empty() => opts.gradient = Some(val.to_string()),
            "covariance" => opts.covariance = parse_bool(val),
            "threads"    => opts.threads = val.parse().ok(),
            _ => {}
        }
    }
    opts
}

/// Normalises a `[fit_options] method` value into the "+"-joined chain
/// format used throughout ferxgui and the run pipeline (e.g. `"saem+imp"`),
/// which is what gets split back apart by `strsplit(method_raw, "\\+")` in
/// the R run script.
///
/// The DSL also allows bracket-array syntax for a method chain — matching
/// the convention already used for `[initial_values]` (`theta = [0.2, 10.0,
/// 1.5]`) — e.g. `method = [saem, imp]`. Passing that bracketed text through
/// unprocessed produced a single malformed method string ("[saem, imp]")
/// that `ferx_fit()`'s `match.arg`-based validation rejected outright
/// (reported: chained methods declared this way failed to run at all).
/// A bare, non-bracketed value (e.g. `method = focei`) passes through
/// unchanged.
fn normalise_method_chain(val: &str) -> String {
    match val.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        Some(inner) => inner
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("+"),
        None => val.to_string(),
    }
}

/// Parse the `[data]` block of a `.ferx` file, if present — the model's own
/// declared dataset path (`path = <path-to-csv>`, resolved relative to the
/// model file's own directory), analogous to NONMEM's `$DATA` record.
/// `None` when the block or key is absent/commented out.
pub fn parse_data_path(source: &str) -> Option<String> {
    let mut in_section = false;
    let mut path = None;
    for line in source.lines() {
        let trimmed = line.trim();
        if is_comment_line(trimmed) || trimmed.is_empty() {
            continue;
        }
        if is_section_header(trimmed) {
            in_section = section_name(trimmed) == Some("data");
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, val)) = trimmed.split_once('=') else { continue; };
        let val = strip_inline_comment(val).trim();
        if key.trim() == "path" && !val.is_empty() {
            path = Some(val.to_string());
        }
    }
    path
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true"  | "t" | "yes" | "1" => Some(true),
        "false" | "f" | "no"  | "0" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Section detection
// ---------------------------------------------------------------------------

/// True for any `[...]` bracketed line, recognised or not — used to decide
/// when to reset `current_section`, independently of whether `section_name`
/// knows the name inside the brackets.
fn is_section_header(line: &str) -> bool {
    super::ferx_grammar::section_header(line).is_some()
}

fn section_name(line: &str) -> Option<&'static str> {
    let inner = super::ferx_grammar::section_header(line)?;
    match inner {
        "parameters" => Some("parameters"),
        "individual_parameters" => Some("individual_parameters"),
        "structural_model" => Some("structural_model"),
        "error_model" => Some("error_model"),
        "fit_options" => Some("fit_options"),
        // Rejected by ferx (E_DEPRECATED_BLOCK); recognised only for section tracking.
        "initial_values" => Some("initial_values"),
        "priors" => Some("priors"),
        "odes" => Some("odes"),
        "simulation" => Some("simulation"),
        "scaling" => Some("scaling"),
        "diffusion" => Some("diffusion"),
        "covariate_nn" => Some("covariate_nn"),
        // ferx-core 0.2.0 additions (joint PK-TTE, adaptive dosing,
        // parameter-dependent ODE baselines, FREM covariates). No dedicated
        // field extraction yet — recognising them here is enough to keep
        // section-boundary tracking correct.
        "event_model" => Some("event_model"),
        "adaptive_dosing" => Some("adaptive_dosing"),
        "initial_conditions" => Some("initial_conditions"),
        "covariates" => Some("covariates"),
        "data" => Some("data"),
        // ferx-core 0.3.0 addition (categorical/binary endpoints). Same as
        // above: recognised for section-boundary tracking, no dedicated field
        // extraction.
        "binary_model" => Some("binary_model"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// [parameters] parsing
// ---------------------------------------------------------------------------

fn parse_parameter_line(line: &str, p: &mut ParsedParams) {
    let line = strip_inline_comment(line);
    let tokens: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
    if tokens.len() < 2 {
        return;
    }
    let rest = tokens[1].trim();
    match tokens[0] {
        "theta" => parse_theta_param(rest, p),
        "omega" => parse_variance_param(rest, true, p),
        "sigma" => parse_variance_param(rest, false, p),
        "block_omega" => parse_block_variance(rest, true, p),
        "block_sigma" => parse_block_variance(rest, false, p),
        _ => {}
    }
}

/// Index of the matching `)` for the `(` at byte `open` (balanced), if any.
fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => { depth -= 1; if depth == 0 { return Some(open + i); } }
            _ => {}
        }
    }
    None
}

/// Splits a trailing `prior(value, rse = X%)` off `s`. Returns the text with the
/// declaration removed, plus `(value, rse_pct)` when one was found.
fn split_prior(s: &str) -> (String, Option<(f64, f64)>) {
    let Some(at) = s.find("prior(").or_else(|| s.find("prior (")) else {
        return (s.to_string(), None);
    };
    let open = at + s[at..].find('(').unwrap_or(0);
    let close = matching_paren(s, open).unwrap_or(s.len().saturating_sub(1));
    let inner = &s[open + 1..close.max(open + 1)];
    let mut value = f64::NAN;
    let mut rse = f64::NAN;
    for (k, part) in inner.split(',').enumerate() {
        let part = part.trim();
        if let Some((key, v)) = part.split_once('=') {
            if key.trim() == "rse" {
                rse = v.trim().trim_end_matches('%').trim().parse().unwrap_or(f64::NAN);
            }
        } else if k == 0 {
            value = part.parse().unwrap_or(f64::NAN);
        }
    }
    let mut rest = s[..at].to_string();
    if close + 1 < s.len() { rest.push_str(&s[close + 1..]); }
    (rest, Some((value, rse)))
}

/// Parse `TVCL(0.134, 0.001, 10.0)`, `TVCL(0.134)`, `TVCL`, any of those followed by
/// `prior(0.15, rse = 10%)`, and the level-block form `NAME[COL, ...](init, lo, hi)`.
fn parse_theta_param(rest: &str, p: &mut ParsedParams) {
    let (rest, prior) = split_prior(rest);
    let rest = rest.trim();
    let name_end = rest.find(|c: char| c == '(' || c == '[' || c.is_whitespace()).unwrap_or(rest.len());
    let name = rest[..name_end].to_owned();
    if name.is_empty() { return; }
    let mut tail = rest[name_end..].trim_start();

    // Level block: NAME[STUDY, TIME]  (or the counted form NAME[N]).
    if tail.starts_with('[') {
        if let Some(close) = tail.find(']') {
            p.theta_level_blocks.push(name.clone());
            tail = tail[close + 1..].trim_start();
        }
    }

    let vals: Vec<f64> = if tail.starts_with('(') {
        let close = matching_paren(tail, 0).unwrap_or(tail.len());
        tail[1..close.max(1)].split(',').filter_map(|s| s.trim().parse::<f64>().ok()).collect()
    } else {
        vec![]
    };
    p.theta_names.push(name.clone());
    p.theta_init.push(*vals.first().unwrap_or(&f64::NAN));
    p.theta_lower.push(*vals.get(1).unwrap_or(&f64::NEG_INFINITY));
    p.theta_upper.push(*vals.get(2).unwrap_or(&f64::INFINITY));
    if let Some((value, rse_pct)) = prior {
        p.priors.push(DeclaredPrior { name, value, rse_pct });
    }
}

/// Parse `ETA_CL ~ 0.07` (variance after `~`), tolerating `(sd)` / `FIX` tails and a
/// trailing `prior(...)`.
fn parse_variance_param(rest: &str, is_omega: bool, p: &mut ParsedParams) {
    let (rest, prior) = split_prior(rest);
    let (name, init) = match rest.find('~') {
        Some(tilde) => {
            let val = rest[tilde + 1..].split_whitespace().next().unwrap_or("");
            (rest[..tilde].trim().to_owned(), val.parse().unwrap_or(f64::NAN))
        }
        None => (rest.trim().to_owned(), f64::NAN),
    };
    if name.is_empty() { return; }
    if let Some((value, rse_pct)) = prior {
        p.priors.push(DeclaredPrior { name: name.clone(), value, rse_pct });
    }
    let (names, inits) = if is_omega { (&mut p.omega_names, &mut p.omega_init) }
                         else { (&mut p.sigma_names, &mut p.sigma_init) };
    names.push(name);
    inits.push(init);
}

/// Parse `block_omega (ETA_CL, ETA_V) = [0.07, 0.02, 0.02]`. The values are the lower
/// triangle row by row; the diagonal entries become the per-name initial variances so
/// the names line up with the fit's omega order.
fn parse_block_variance(rest: &str, is_omega: bool, p: &mut ParsedParams) {
    let Some(open) = rest.find('(') else { return };
    let Some(close) = matching_paren(rest, open) else { return };
    let names: Vec<String> = rest[open + 1..close].split(',')
        .map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
    let vals = rest[close + 1..].split_once('=')
        .map(|(_, v)| parse_bracket_list(v)).unwrap_or_default();
    let (nn, ii) = if is_omega { (&mut p.omega_names, &mut p.omega_init) }
                   else { (&mut p.sigma_names, &mut p.sigma_init) };
    for (k, name) in names.into_iter().enumerate() {
        nn.push(name);
        ii.push(vals.get(k * (k + 1) / 2 + k).copied().unwrap_or(f64::NAN));
    }
}

/// `[priors]` section: `from_fit = "path/to/model.fitrx"`.
fn parse_priors_line(line: &str, p: &mut ParsedParams) {
    let line = strip_inline_comment(line);
    if let Some((key, val)) = line.split_once('=') {
        if key.trim() == "from_fit" {
            let v = val.trim().trim_matches('"').trim_matches('\'').trim();
            if !v.is_empty() { p.priors_from_fit = Some(v.to_owned()); }
        }
    }
}

fn parse_bracket_list(s: &str) -> Vec<f64> {
    s.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|t| t.trim().parse::<f64>().ok())
        .collect()
}

/// True for a full-line comment (`#` or `//`).
fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with('#') || trimmed.starts_with("//")
}

/// Cuts a line at the first `#` or `//` comment marker.
fn strip_inline_comment(line: &str) -> &str {
    let mut cut = line.len();
    if let Some(i) = line.find('#') { cut = cut.min(i); }
    if let Some(i) = line.find("//") { cut = cut.min(i); }
    &line[..cut]
}

// ---------------------------------------------------------------------------
// Syntax token types (used by the editor tokenizer)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// `[section_name]`
    SectionHeader,
    /// `theta`, `omega`, `sigma`, `block_omega`
    ParamKeyword,
    /// `pk`, `one_cpt_oral`, `two_cpt_oral`, `one_cpt_iv`, `two_cpt_iv`, etc.
    BuiltinFunction,
    /// `method`, `maxiter`, `covariance`, `gradient`, `threads`
    OptionKey,
    /// A number literal
    Number,
    /// `# …` or `// …` to end of line
    Comment,
    /// Everything else
    Plain,
}

/// Tokenise a single line for the editor colour pass.
/// Returns (start_byte, end_byte, kind) triples.
pub fn tokenise_line(line: &str) -> Vec<(usize, usize, TokenKind)> {
    let mut out = Vec::new();
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();

    // Whole-line comment.
    if is_comment_line(trimmed) {
        out.push((indent, line.len(), TokenKind::Comment));
        return out;
    }

    // Section header `[…]`.
    if trimmed.starts_with('[') {
        if let (Some(_), Some(end)) = (super::ferx_grammar::section_header(trimmed), trimmed.find(']')) {
            out.push((indent, indent + end + 1, TokenKind::SectionHeader));
        }
        return out;
    }

    // Walk char-by-char.
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Inline comment.
        if bytes[i] == b'#' || (bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/')) {
            out.push((i, line.len(), TokenKind::Comment));
            break;
        }
        // Number.
        if bytes[i].is_ascii_digit() || (bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()) {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'e' || bytes[i] == b'E' || bytes[i] == b'+' || bytes[i] == b'-') {
                i += 1;
            }
            out.push((start, i, TokenKind::Number));
            continue;
        }
        // Identifier.
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let kind = classify_word(word);
            out.push((start, i, kind));
            continue;
        }
        i += 1;
    }

    out
}

fn classify_word(w: &str) -> TokenKind {
    match w {
        "theta" | "omega" | "sigma" | "block_omega" | "block_sigma" | "kappa"
        | "block_kappa" | "prior" => TokenKind::ParamKeyword,
        "one_cpt_oral" | "two_cpt_oral" | "three_cpt_oral" | "three_cpt_infusion"
        | "pk" | "ode" | "ode_template" | "power" => TokenKind::BuiltinFunction,
        "method" | "maxiter" | "covariance" | "gradient" | "threads"
        | "output" | "optimizer" | "interaction" | "lloq"
        | "lagtime" | "alag" | "obs_scale" | "sir" | "bloq_method"
        | "reconverge_gradient_interval" | "stagnation_guard" | "optimizer_trace"
        // ferx-core 0.3.0: [fit_options] ode_method, [binary_model] keys.
        | "ode_method" | "cmt" | "logit"
        // ferx-core 0.4.0: SAEM/IMP [fit_options] keys and the [priors] key.
        | "scale_adaptation" | "scale_deadband" | "mstep_solver" | "mstep_draws"
        | "mstep_damping" | "n_mh_steps" | "from_fit" => TokenKind::OptionKey,
        _ => TokenKind::Plain,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_keywords_not_builtin() {
        for w in ["one_cpt_iv_bolus", "one_cpt_infusion", "two_cpt_iv_bolus",
                  "two_cpt_infusion", "three_cpt_iv_bolus"] {
            assert!(!matches!(classify_word(w), TokenKind::BuiltinFunction), "{w}");
        }
        assert!(matches!(classify_word("one_cpt_oral"), TokenKind::BuiltinFunction));
    }

    const WARFARIN: &str = r#"
# One-compartment oral PK model (warfarin)

[parameters]
  theta TVCL(0.134, 0.001, 10.0)
  theta TVV(8.1, 0.1, 500.0)
  theta TVKA(1.0, 0.01, 50.0)
  omega ETA_CL ~ 0.07
  omega ETA_V  ~ 0.02
  sigma PROP_ERR ~ 0.01
"#;

    #[test]
    fn unrecognised_section_does_not_leak_into_next_recognised_one() {
        // An unrecognised section (e.g. a newer ferx-core DSL block) between
        // two [parameters] blocks must not let the second block's lines be
        // parsed twice, nor let the unrecognised block's own content be
        // misread as parameter declarations.
        let src = "\
[parameters]
  theta TVCL(0.1, 0.001, 10.0)

[event_model]
  hazard = H0 * exp(BETA * (central / V))

[parameters]
  theta TVV(5.0, 0.1, 100.0)
";
        let p = parse_params(src);
        assert_eq!(p.theta_names, vec!["TVCL", "TVV"]);
    }

    #[test]
    fn new_dsl_sections_are_recognised() {
        for name in ["event_model", "adaptive_dosing", "initial_conditions", "covariates", "binary_model", "priors"] {
            assert_eq!(section_name(&format!("[{name}]")), Some(name));
        }
    }

    #[test]
    fn fit_options_reads_explicit_values() {
        let src = "[fit_options]\n  method = focei\n  covariance = true\n  gradient = ad\n  threads = 4\n";
        let o = parse_fit_options(src);
        assert_eq!(o.method.as_deref(), Some("focei"));
        assert_eq!(o.covariance, Some(true));
        assert_eq!(o.gradient.as_deref(), Some("ad"));
        assert_eq!(o.threads, Some(4));
    }

    #[test]
    fn fit_options_commented_covariance_reads_as_absent() {
        // The reported bug: commenting the directive must disable it, not be ignored.
        let src = "[fit_options]\n  method = foce\n#  covariance = true\n";
        let o = parse_fit_options(src);
        assert_eq!(o.covariance, None, "commented directive must read as absent");
        assert_eq!(o.method.as_deref(), Some("foce"));
    }

    #[test]
    fn fit_options_strips_inline_comment_and_parses_false() {
        let src = "[fit_options]\n  covariance = false  # no SE step\n";
        assert_eq!(parse_fit_options(src).covariance, Some(false));
    }

    #[test]
    fn fit_options_method_chain_bracket_syntax_normalises_to_plus_joined() {
        // Regression test: this bracketed method chain previously passed
        // through as the literal string "[saem, imp]", which `ferx_fit()`
        // rejected outright (not a recognised method) instead of being
        // treated as a two-step saem-then-imp chain.
        let src = "[fit_options]\n  method = [saem, imp]\n";
        assert_eq!(parse_fit_options(src).method.as_deref(), Some("saem+imp"));
    }

    #[test]
    fn fit_options_method_bare_value_is_unaffected() {
        let src = "[fit_options]\n  method = focei\n";
        assert_eq!(parse_fit_options(src).method.as_deref(), Some("focei"));
    }

    #[test]
    fn fit_options_method_chain_bracket_syntax_handles_extra_whitespace() {
        let src = "[fit_options]\n  method = [ saem ,  imp  ]\n";
        assert_eq!(parse_fit_options(src).method.as_deref(), Some("saem+imp"));
    }

    #[test]
    fn data_path_reads_explicit_value() {
        let src = "[data]\n  path = warfarin.csv\n";
        assert_eq!(parse_data_path(src).as_deref(), Some("warfarin.csv"));
    }

    #[test]
    fn data_path_absent_when_no_data_block() {
        let src = "[parameters]\n  theta TVCL(0.134, 0.001, 10.0)\n";
        assert_eq!(parse_data_path(src), None);
    }

    #[test]
    fn data_path_commented_out_reads_as_absent() {
        let src = "[data]\n#  path = warfarin.csv\n";
        assert_eq!(parse_data_path(src), None, "commented directive must read as absent");
    }

    #[test]
    fn data_path_strips_inline_comment() {
        let src = "[data]\n  path = warfarin.csv  # primary dataset\n";
        assert_eq!(parse_data_path(src).as_deref(), Some("warfarin.csv"));
    }

    #[test]
    fn data_path_does_not_leak_into_other_sections() {
        // A `path = ...` line outside [data] (e.g. in an unrelated section)
        // must not be picked up.
        let src = "[parameters]\n  theta TVCL(0.134, 0.001, 10.0)\n[fit_options]\n  method = foce\n";
        assert_eq!(parse_data_path(src), None);
    }

    #[test]
    fn parses_theta_names_and_inits() {
        let p = parse_params(WARFARIN);
        assert_eq!(p.theta_names, vec!["TVCL", "TVV", "TVKA"]);
        assert!((p.theta_init[0] - 0.134).abs() < 1e-9);
        assert!((p.theta_lower[0] - 0.001).abs() < 1e-9);
        assert!((p.theta_upper[0] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn parses_omega_and_sigma() {
        let p = parse_params(WARFARIN);
        assert_eq!(p.omega_names, vec!["ETA_CL", "ETA_V"]);
        assert!((p.omega_init[0] - 0.07).abs() < 1e-9);
        assert_eq!(p.sigma_names, vec!["PROP_ERR"]);
        assert!((p.sigma_init[0] - 0.01).abs() < 1e-9);
    }

    #[test]
    fn description_from_first_comment() {
        let p = parse_params(WARFARIN);
        assert_eq!(p.description, "One-compartment oral PK model (warfarin)");
    }

    #[test]
    fn tokenise_section_header() {
        let toks = tokenise_line("[parameters]");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].2, TokenKind::SectionHeader);
    }

    #[test]
    fn tokenise_theta_line() {
        let toks = tokenise_line("  theta TVCL(0.134, 0.001, 10.0)");
        let kinds: Vec<_> = toks.iter().map(|(_, _, k)| k.clone()).collect();
        assert!(kinds.contains(&TokenKind::ParamKeyword));
        assert!(kinds.contains(&TokenKind::Number));
    }

    #[test]
    fn initial_values_block_is_not_applied() {
        // ferx rejects [initial_values] (E_DEPRECATED_BLOCK); the GUI must keep showing
        // the [parameters] inits, which are what the engine would have used.
        let src = format!("{WARFARIN}\n[initial_values]\n  theta = [0.2, 10.0, 1.5]\n  omega = [0.09, 0.04]\n  sigma = [0.02]\n");
        let p = parse_params(&src);
        assert!((p.theta_init[0] - 0.134).abs() < 1e-9);
        assert!((p.omega_init[0] - 0.07).abs() < 1e-9);
        assert!((p.sigma_init[0] - 0.01).abs() < 1e-9);
    }

    #[test]
    fn slash_slash_is_a_comment() {
        let p = parse_params("[parameters]\n  theta TVCL(1, 0, 5) // clearance note\n  // theta GONE(1, 0, 5)\n  omega ETA_CL ~ 0.1 // iiv\n");
        assert_eq!(p.theta_names, vec!["TVCL"]);
        assert_eq!(p.theta_upper, vec![5.0]);
        assert_eq!(p.omega_init, vec![0.1]);
        let toks = tokenise_line("  theta X(1, 0, 5) // note");
        assert_eq!(toks.last().unwrap().2, TokenKind::Comment);
        assert_eq!(tokenise_line("// whole line")[0].2, TokenKind::Comment);
    }

    #[test]
    fn theta_prior_does_not_break_bounds() {
        let p = parse_params("[parameters]\n  theta TVCL(0.134, 0.001, 10.0) prior(0.15, rse = 10%)\n  theta TVV(8, 0.1, 500)\n");
        assert_eq!(p.theta_names, vec!["TVCL", "TVV"]);
        assert!((p.theta_upper[0] - 10.0).abs() < 1e-9);
        assert_eq!(p.priors, vec![DeclaredPrior { name: "TVCL".into(), value: 0.15, rse_pct: 10.0 }]);
    }

    #[test]
    fn omega_and_sigma_priors_and_sd_tail() {
        let p = parse_params("[parameters]\n  omega ETA_CL ~ 0.07 prior(0.1, rse = 30%)\n  sigma PROP ~ 0.01 (sd)\n");
        assert_eq!(p.omega_init, vec![0.07]);
        assert_eq!(p.sigma_init, vec![0.01]);
        assert_eq!(p.priors.len(), 1);
        assert_eq!(p.priors[0].name, "ETA_CL");
    }

    #[test]
    fn block_omega_names_and_diagonals_align_with_fit_order() {
        let p = parse_params("[parameters]\n  block_omega (ETA_CL, ETA_V) = [0.07, 0.02, 0.03]\n  omega ETA_KA ~ 0.40\n");
        assert_eq!(p.omega_names, vec!["ETA_CL", "ETA_V", "ETA_KA"]);
        assert_eq!(p.omega_init, vec![0.07, 0.03, 0.40]);
    }

    #[test]
    fn level_block_theta_is_matched_by_base_name() {
        let p = parse_params("[parameters]\n  theta PLACEBO[STUDY, TIME](0.0, -5, 5)\n  theta E0(1, 0, 10)\n");
        assert_eq!(p.theta_names, vec!["PLACEBO", "E0"]);
        assert!(p.has_level_block());
        assert_eq!(p.theta_init_for("PLACEBO[STUDY=1,TIME=1]", 0), 0.0);
        assert_eq!(p.theta_init_for("E0", 5), 1.0);
        assert!(p.theta_init_for("UNKNOWN", 0).is_nan());
    }

    #[test]
    fn priors_from_fit_is_read() {
        let p = parse_params("[priors]\n  from_fit = \"base.fitrx\"  # earlier study\n");
        assert_eq!(p.priors_from_fit.as_deref(), Some("base.fitrx"));
    }

    #[test]
    fn tokenise_new_040_tokens() {
        for (line, kind) in [
            ("  scale_adaptation = robbins_monro", TokenKind::OptionKey),
            ("  n_mh_steps = 10", TokenKind::OptionKey),
            ("  from_fit = \"a.fitrx\"", TokenKind::OptionKey),
            ("  ode_template two_cpt_oral(cl=CL)", TokenKind::BuiltinFunction),
            ("  DV ~ power(PROP_ERR, 0.7)", TokenKind::BuiltinFunction),
            ("  block_sigma (A, B) = [1, 0, 1]", TokenKind::ParamKeyword),
        ] {
            assert!(tokenise_line(line).iter().any(|t| t.2 == kind), "{line}");
        }
    }

    const CORPUS_BODY: &str = "
  theta TVCL(0.134, 0.001, 10.0)
  theta TVV(8.0, 0.1, 500.0)
  omega ETA_CL ~ 0.07
  sigma PROP_ERR ~ 0.01
[fit_options]HDR2
  method = focei
  covariance = false
";

    #[test]
    fn header_corpus_matches_ferx() {
        // (parameters header, fit_options header, ferx accepts, expected theta count)
        let cases = [
            ("[parameters]", "", true, 2),
            ("[parameters]  # main", "  # opts", true, 2),
            ("[parameters]  // main", "  // opts", true, 2),
            ("[parameters]\t# main", "\t# opts", true, 2),
            ("[parameters]#main", "#opts", true, 2),
            ("[ parameters ]", "", false, 0),
        ];
        for (h, h2, ok, n) in cases {
            let src = format!("{h}{}", CORPUS_BODY.replace("HDR2", h2));
            let p = parse_params(&src);
            assert_eq!(p.theta_names.len(), n, "theta count for {h:?}");
            if ok {
                assert_eq!(p.omega_names.len(), 1, "{h:?}");
                assert_eq!(p.sigma_names.len(), 1, "{h:?}");
                let o = parse_fit_options(&src);
                assert_eq!(o.method.as_deref(), Some("focei"), "{h:?}");
                assert_eq!(o.covariance, Some(false), "{h:?}");
            }
        }
    }
}
