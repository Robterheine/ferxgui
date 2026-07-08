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
///
///   [initial_values]                    # overrides the defaults in [parameters]
///     theta = [0.2, 10.0, 1.5]
///     omega = [0.09, 0.04, 0.30]
///     sigma = [0.02]
use crate::domain::ParsedParams;

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
        if trimmed.starts_with('#') || trimmed.is_empty() {
            // First non-empty comment becomes the description.
            if p.description.is_empty() && trimmed.starts_with('#') {
                let desc = trimmed.trim_start_matches('#').trim();
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
            Some("initial_values") => parse_initial_values_line(trimmed, &mut p),
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
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(sec) = section_name(trimmed) {
            in_section = sec == "fit_options";
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, val)) = trimmed.split_once('=') else { continue; };
        // Strip any inline comment from the value.
        let val = val.split('#').next().unwrap_or("").trim();
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
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(sec) = section_name(trimmed) {
            in_section = sec == "data";
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, val)) = trimmed.split_once('=') else { continue; };
        let val = val.split('#').next().unwrap_or("").trim();
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
    line.starts_with('[') && line.ends_with(']')
}

fn section_name(line: &str) -> Option<&'static str> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    match inner {
        "parameters" => Some("parameters"),
        "individual_parameters" => Some("individual_parameters"),
        "structural_model" => Some("structural_model"),
        "error_model" => Some("error_model"),
        "fit_options" => Some("fit_options"),
        "initial_values" => Some("initial_values"),
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
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// [parameters] parsing
// ---------------------------------------------------------------------------

fn parse_parameter_line(line: &str, p: &mut ParsedParams) {
    // Strip inline comment.
    let line = strip_inline_comment(line);
    let tokens: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
    if tokens.len() < 2 {
        return;
    }
    match tokens[0] {
        "theta" => parse_theta_param(tokens[1].trim(), p),
        "omega" => parse_variance_param(tokens[1].trim(), &mut p.omega_names, &mut p.omega_init),
        "sigma" => parse_variance_param(tokens[1].trim(), &mut p.sigma_names, &mut p.sigma_init),
        _ => {}
    }
}

/// Parse `TVCL(0.134, 0.001, 10.0)` or `TVCL(0.134)` or just `TVCL`.
fn parse_theta_param(rest: &str, p: &mut ParsedParams) {
    if let Some(paren) = rest.find('(') {
        let name = rest[..paren].trim().to_owned();
        let inside = rest[paren + 1..].trim_end_matches(')');
        let vals: Vec<f64> = inside
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();
        p.theta_names.push(name);
        p.theta_init.push(*vals.first().unwrap_or(&f64::NAN));
        p.theta_lower.push(*vals.get(1).unwrap_or(&f64::NEG_INFINITY));
        p.theta_upper.push(*vals.get(2).unwrap_or(&f64::INFINITY));
    } else {
        p.theta_names.push(rest.trim().to_owned());
        p.theta_init.push(f64::NAN);
        p.theta_lower.push(f64::NEG_INFINITY);
        p.theta_upper.push(f64::INFINITY);
    }
}

/// Parse `ETA_CL ~ 0.07` (variance after `~`).
fn parse_variance_param(rest: &str, names: &mut Vec<String>, inits: &mut Vec<f64>) {
    if let Some(tilde) = rest.find('~') {
        let name = rest[..tilde].trim().to_owned();
        let val: f64 = rest[tilde + 1..].trim().parse().unwrap_or(f64::NAN);
        names.push(name);
        inits.push(val);
    } else {
        names.push(rest.trim().to_owned());
        inits.push(f64::NAN);
    }
}

// ---------------------------------------------------------------------------
// [initial_values] parsing
// ---------------------------------------------------------------------------

fn parse_initial_values_line(line: &str, p: &mut ParsedParams) {
    // Expect: `theta = [0.2, 10.0, 1.5]`  or  `omega = [0.09]`
    let line = strip_inline_comment(line);
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() < 2 {
        return;
    }
    let key = parts[0].trim();
    let vals = parse_bracket_list(parts[1]);
    match key {
        "theta" => {
            for (i, v) in vals.iter().enumerate() {
                if let Some(slot) = p.theta_init.get_mut(i) {
                    *slot = *v;
                }
            }
        }
        "omega" => {
            for (i, v) in vals.iter().enumerate() {
                if let Some(slot) = p.omega_init.get_mut(i) {
                    *slot = *v;
                }
            }
        }
        "sigma" => {
            for (i, v) in vals.iter().enumerate() {
                if let Some(slot) = p.sigma_init.get_mut(i) {
                    *slot = *v;
                }
            }
        }
        _ => {}
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

fn strip_inline_comment(line: &str) -> &str {
    if let Some(pos) = line.find('#') {
        &line[..pos]
    } else {
        line
    }
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
    /// `# …` to end of line
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
    if trimmed.starts_with('#') {
        out.push((indent, line.len(), TokenKind::Comment));
        return out;
    }

    // Section header `[…]`.
    if trimmed.starts_with('[') {
        if let Some(end) = trimmed.find(']') {
            out.push((indent, indent + end + 1, TokenKind::SectionHeader));
        }
        return out;
    }

    // Walk char-by-char.
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Inline comment.
        if bytes[i] == b'#' {
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
        "theta" | "omega" | "sigma" | "block_omega" | "kappa" => TokenKind::ParamKeyword,
        "one_cpt_oral" | "one_cpt_iv_bolus" | "one_cpt_infusion"
        | "two_cpt_oral" | "two_cpt_iv_bolus" | "two_cpt_infusion"
        | "three_cpt_oral" | "three_cpt_iv_bolus" | "three_cpt_infusion"
        | "pk" | "ode" => TokenKind::BuiltinFunction,
        "method" | "maxiter" | "covariance" | "gradient" | "threads"
        | "output" | "optimizer" | "interaction" | "lloq"
        | "lagtime" | "alag" | "obs_scale" | "sir" | "bloq_method"
        | "reconverge_gradient_interval" | "stagnation_guard" | "optimizer_trace" => TokenKind::OptionKey,
        _ => TokenKind::Plain,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const WARFARIN: &str = r#"
# One-compartment oral PK model (warfarin)

[parameters]
  theta TVCL(0.134, 0.001, 10.0)
  theta TVV(8.1, 0.1, 500.0)
  theta TVKA(1.0, 0.01, 50.0)
  omega ETA_CL ~ 0.07
  omega ETA_V  ~ 0.02
  sigma PROP_ERR ~ 0.01

[initial_values]
  theta = [0.2, 10.0, 1.5]
  omega = [0.09, 0.04]
  sigma = [0.02]
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
        for name in ["event_model", "adaptive_dosing", "initial_conditions", "covariates"] {
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
        assert!((p.theta_init[0] - 0.2).abs() < 1e-9, "init_values override expected");
        assert!((p.theta_lower[0] - 0.001).abs() < 1e-9);
        assert!((p.theta_upper[0] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn parses_omega_and_sigma() {
        let p = parse_params(WARFARIN);
        assert_eq!(p.omega_names, vec!["ETA_CL", "ETA_V"]);
        assert!((p.omega_init[0] - 0.09).abs() < 1e-9);
        assert_eq!(p.sigma_names, vec!["PROP_ERR"]);
        assert!((p.sigma_init[0] - 0.02).abs() < 1e-9);
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
}
