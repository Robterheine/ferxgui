/// Lightweight system notification — no extra crates, uses OS-native tools.
///
/// * macOS  — `osascript` (always available)
/// * Linux  — `notify-send` (common on GNOME/KDE; silently skipped if absent)
/// * Windows — PowerShell `New-BurntToastNotification` or a msg box fallback

pub fn send(model_stem: &str, success: bool) {
    let title = "FeRx GUI";
    let body = if success {
        format!("✓  {model_stem} completed")
    } else {
        format!("✗  {model_stem} failed")
    };

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            r#"display notification "{body}" with title "{title}""#
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
        // PowerShell balloon / toast — works without extra crates.
        let script = format!(
            r#"[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; `
               $t = [Windows.UI.Notifications.ToastTemplateType]::ToastText02; `
               $x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent($t); `
               $x.GetElementsByTagName('text')[0].AppendChild($x.CreateTextNode('{title}')) | Out-Null; `
               $x.GetElementsByTagName('text')[1].AppendChild($x.CreateTextNode('{body}')) | Out-Null; `
               [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('FeRxGUI').Show([Windows.UI.Notifications.ToastNotification]::new($x))"#
        );
        std::thread::spawn(move || {
            let mut cmd = std::process::Command::new("powershell");
            cmd.args(["-Command", &script]);
            let _ = crate::io::r_extract::apply_no_window(cmd).output();
        });
    }

    // Suppress unused-variable warnings on platforms that don't use body/title.
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (title, body);
    }
}
