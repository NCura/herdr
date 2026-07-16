//! Session environment forwarded from attaching clients.
//!
//! Panes inherit the server process environment, but a daemonized server can
//! outlive (or never have seen) the graphical session the user is attaching
//! from — e.g. a server respawned over SSH has no `WAYLAND_DISPLAY`, so
//! clipboard tools inside panes cannot reach the compositor. Attaching local
//! clients forward a snapshot of their display environment
//! (`platform::attach_forwarded_env`), and pane spawns overlay the latest
//! snapshot on top of the inherited environment, tmux `update-environment`
//! style. Runtime-only state: never persisted, latest snapshot wins.

use std::sync::Mutex;

use portable_pty::CommandBuilder;

static CLIENT_VARS: Mutex<Vec<(String, Option<String>)>> = Mutex::new(Vec::new());

fn lock_client_vars() -> std::sync::MutexGuard<'static, Vec<(String, Option<String>)>> {
    CLIENT_VARS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Replaces the stored snapshot with the latest client-provided one.
pub(crate) fn update_from_client(vars: Vec<(String, Option<String>)>) {
    let mut stored = lock_client_vars();
    if *stored != vars {
        tracing::info!(
            vars = ?vars.iter().map(|(name, value)| (name.as_str(), value.is_some())).collect::<Vec<_>>(),
            "session environment updated from client"
        );
    }
    *stored = vars;
}

/// Overlays the stored snapshot on a pane spawn command: present variables are
/// set, absent ones are removed so panes never see values staler than the most
/// recently attached client.
pub(crate) fn apply_to_command(cmd: &mut CommandBuilder) {
    for (name, value) in lock_client_vars().iter() {
        match value {
            Some(value) => cmd.env(name, value),
            None => cmd.env_remove(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_sets_and_removes_snapshot_vars() {
        let mut cmd = CommandBuilder::new("true");
        cmd.env("DISPLAY", ":0");
        update_from_client(vec![
            ("WAYLAND_DISPLAY".to_owned(), Some("wayland-1".to_owned())),
            ("DISPLAY".to_owned(), None),
        ]);

        apply_to_command(&mut cmd);

        assert_eq!(
            cmd.get_env("WAYLAND_DISPLAY"),
            Some(std::ffi::OsStr::new("wayland-1"))
        );
        assert_eq!(cmd.get_env("DISPLAY"), None);
    }

    #[test]
    fn later_snapshot_replaces_earlier_one() {
        update_from_client(vec![(
            "WAYLAND_DISPLAY".to_owned(),
            Some("wayland-0".to_owned()),
        )]);
        update_from_client(vec![(
            "WAYLAND_DISPLAY".to_owned(),
            Some("wayland-1".to_owned()),
        )]);

        let mut cmd = CommandBuilder::new("true");
        apply_to_command(&mut cmd);
        assert_eq!(
            cmd.get_env("WAYLAND_DISPLAY"),
            Some(std::ffi::OsStr::new("wayland-1"))
        );
    }

    #[test]
    fn empty_snapshot_leaves_command_untouched() {
        update_from_client(Vec::new());
        let mut cmd = CommandBuilder::new("true");
        cmd.env("DISPLAY", ":0");
        apply_to_command(&mut cmd);
        assert_eq!(cmd.get_env("DISPLAY"), Some(std::ffi::OsStr::new(":0")));
    }
}
