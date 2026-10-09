/// Lightweight system notification — no extra crates, uses OS-native tools.
///
/// * macOS  — `osascript` (always available)
/// * Linux  — `notify-send` (common on GNOME/KDE; silently skipped if absent)
/// * Windows — a PowerShell toast. Best-effort: Windows may not show it when the app ID is not
///   registered; the run result is always visible in the app itself.
pub fn send(model_stem: &str, success: bool) {
    let title = "FeRx GUI";
    let body = body_text(model_stem, success);

    #[cfg(target_os = "macos")]
    {
        // Escape for AppleScript double-quoted string context.
        let body_esc = body.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            r#"display notification "{body_esc}" with title "{title}""#
        );
        std::thread::spawn(move || {
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output();
        });
    }

    #[cfg(target_os = "linux")]
    {
        std::thread::spawn(move || {
            let _ = std::process::Command::new("notify-send")
                .args([title, &body])
                .output();
        });
    }

    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(move || {
            let mut cmd = std::process::Command::new("powershell");
            cmd.args(["-NoProfile", "-Command", TOAST_PS]);
            set_notify_env(&mut cmd, title, &body);
            let _ = crate::io::r_extract::apply_no_window(cmd).output();
        });
    }

    // Suppress unused-variable warnings on platforms that don't use body/title.
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (title, body);
    }
}

fn body_text(model_stem: &str, success: bool) -> String {
    if success { format!("✓  {model_stem} completed") } else { format!("✗  {model_stem} failed") }
}

/// The toast script is a FIXED constant: user-controlled text (a model stem can contain quotes,
/// `$(...)`, backticks) reaches PowerShell only through environment variables, never as code.
#[cfg(any(target_os = "windows", test))]
const TOAST_PS: &str = "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; \
    $t = [Windows.UI.Notifications.ToastTemplateType]::ToastText02; \
    $x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent($t); \
    $x.GetElementsByTagName('text')[0].AppendChild($x.CreateTextNode($env:FERX_NOTIFY_TITLE)) | Out-Null; \
    $x.GetElementsByTagName('text')[1].AppendChild($x.CreateTextNode($env:FERX_NOTIFY_BODY)) | Out-Null; \
    [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('FeRxGUI').Show([Windows.UI.Notifications.ToastNotification]::new($x))";

#[cfg(target_os = "windows")]
fn set_notify_env(cmd: &mut std::process::Command, title: &str, body: &str) {
    cmd.env("FERX_NOTIFY_TITLE", title).env("FERX_NOTIFY_BODY", body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_script_is_constant_and_takes_text_from_env() {
        // No user text can be spliced into the script: it is a `const`, and it only reads $env:.
        assert!(TOAST_PS.contains("$env:FERX_NOTIFY_BODY") && TOAST_PS.contains("$env:FERX_NOTIFY_TITLE"));
        assert!(!TOAST_PS.contains("{body"));
    }

    #[test]
    fn body_keeps_curly_quotes_unchanged() {
        let stem = "model\u{2018}s \u{2019}x\u{2019}";
        assert_eq!(body_text(stem, true), format!("✓  {stem} completed"));
    }

    /// Windows CI: the body travels through the environment unchanged, including curly quotes and
    /// characters that would break a single-quoted PowerShell string.
    #[cfg(windows)]
    #[test]
    fn windows_env_passthrough_is_exact() {
        let body = body_text("it\u{2019}s \u{2018}q\u{2019} $(whoami) `x` \"y\"", true);
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-Command",
                  "[Console]::OutputEncoding=[Text.Encoding]::UTF8; [Console]::Out.Write($env:FERX_NOTIFY_BODY)"]);
        set_notify_env(&mut cmd, "t", &body);
        let out = cmd.output().expect("powershell runs");
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), body);
    }
}
