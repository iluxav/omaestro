//! Hyprland: the instance the daemon belongs to, commands through
//! `hyprctl` (`ctl.rs`) and the event socket (`events.rs`).

use std::env;
use std::path::PathBuf;

pub mod ctl;
pub mod events;

pub use ctl::HyprCtl;

/// The Hyprland instance this daemon was started in. Several instances can
/// leave directories under `$XDG_RUNTIME_DIR/hypr/`, so the signature always
/// comes from the environment, never from the first directory found.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    signature: String,
    runtime_dir: PathBuf,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum InstanceError {
    #[error(
        "HYPRLAND_INSTANCE_SIGNATURE is unset: the daemon was started outside the Hyprland session \
         (for example from a plain tty or a systemd unit without the session environment)"
    )]
    SignatureUnset,
    #[error("XDG_RUNTIME_DIR is unset, cannot find Hyprland's sockets")]
    RuntimeDirUnset,
}

impl Instance {
    pub fn from_env() -> Result<Self, InstanceError> {
        Self::from_vars(
            env::var("HYPRLAND_INSTANCE_SIGNATURE").ok(),
            env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        )
    }

    pub fn from_vars(
        signature: Option<String>,
        runtime_dir: Option<PathBuf>,
    ) -> Result<Self, InstanceError> {
        let signature = signature
            .filter(|s| !s.is_empty())
            .ok_or(InstanceError::SignatureUnset)?;
        let runtime_dir = runtime_dir
            .filter(|d| !d.as_os_str().is_empty())
            .ok_or(InstanceError::RuntimeDirUnset)?;
        Ok(Self {
            signature,
            runtime_dir,
        })
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// Where this instance keeps its sockets.
    pub fn dir(&self) -> PathBuf {
        self.runtime_dir.join("hypr").join(&self.signature)
    }

    /// The socket Hyprland writes its events to, one per line.
    pub fn event_socket(&self) -> PathBuf {
        self.dir().join(".socket2.sock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_signature_is_an_error_not_a_guess() {
        for missing in [None, Some(String::new())] {
            let err = Instance::from_vars(missing, Some("/run/user/1000".into())).unwrap_err();
            assert_eq!(err, InstanceError::SignatureUnset);
            assert!(
                err.to_string()
                    .starts_with("HYPRLAND_INSTANCE_SIGNATURE is unset")
            );
        }
    }

    #[test]
    fn sockets_live_under_the_signature_from_the_environment() {
        let instance =
            Instance::from_vars(Some("abc_1_2".into()), Some("/run/user/1000".into())).unwrap();
        assert_eq!(instance.signature(), "abc_1_2");
        assert_eq!(
            instance.event_socket(),
            PathBuf::from("/run/user/1000/hypr/abc_1_2/.socket2.sock")
        );
        assert_eq!(
            Instance::from_vars(Some("abc".into()), None).unwrap_err(),
            InstanceError::RuntimeDirUnset
        );
    }
}
