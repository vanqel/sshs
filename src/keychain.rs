use anyhow::{Context, Result};
use std::process::{Command, Stdio};

/// Retrieve from OS keychain (macOS: Keychain, Linux: secret-tool).
pub fn retrieve_from_keychain(alias: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-a",
                alias,
                "-s",
                "sshs-keychain",
                "-w",
            ])
            .output()
            .context("Failed to run security command")?;
        if !output.status.success() {
            anyhow::bail!("Keychain lookup failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .to_string()
            .trim()
            .to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let output = Command::new("secret-tool")
            .args(["lookup", "application", "purple-ssh", "host", alias])
            .output()
            .context("Failed to run secret-tool")?;
        if !output.status.success() {
            anyhow::bail!("Secret-tool lookup failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// Store a password in the OS keychain.
pub fn store_in_keychain(alias: &str, password: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("security")
            .args([
                "add-generic-password",
                "-U",
                "-a",
                alias,
                "-s",
                "sshs-keychain",
                "-w",
                password.trim(),
            ])
            .status()
            .context("Failed to run security command")?;
        if !status.success() {
            anyhow::bail!("Failed to store password in Keychain");
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut child = Command::new("secret-tool")
            .args([
                "store",
                "--label",
                &format!("purple-ssh: {}", alias),
                "application",
                "purple-ssh",
                "host",
                alias,
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to run secret-tool")?;
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            stdin.write_all(password.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("Failed to store password with secret-tool");
        }
        Ok(())
    }
}


/// Remove a password from the OS keychain.
pub fn remove_from_keychain(alias: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("security")
            .args(["delete-generic-password", "-a", alias, "-s", "sshs-keychain"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("Failed to run security command")?;
        if !status.success() {
            anyhow::bail!("No password found for '{}' in Keychain", alias);
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let status = Command::new("secret-tool")
            .args(["clear", "application", "purple-ssh", "host", alias])
            .status()
            .context("Failed to run secret-tool")?;
        if !status.success() {
            anyhow::bail!("Failed to remove password with secret-tool");
        }
        Ok(())
    }
}