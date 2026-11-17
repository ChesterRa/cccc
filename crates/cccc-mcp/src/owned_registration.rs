//! Optional process observations for controlled main bindings. Registration is
//! diagnostic and does not grant tool authority or gate MCP initialization.

use cccc_core::HomeLayout;
use cccc_core::owned_mcp::{Binding, Registration};
use std::ffi::OsStr;
use std::io;

pub(super) async fn register(home: &HomeLayout, file: Option<&OsStr>) -> io::Result<()> {
    let Some(file) = file else {
        return Ok(());
    };
    let binding = Binding::read_file(std::path::Path::new(file))?;
    let report = Registration::current(&binding)?;
    cccc_client::owned_mcp::register(home, &binding, &report).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invalid_process_observation_does_not_become_a_valid_registration() {
        let temp = tempfile::tempdir().expect("isolated home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        register(&home, None)
            .await
            .expect("native MCP has no owner dependency");
        let error = register(&home, Some(OsStr::new("not-json")))
            .await
            .expect_err("invalid observation must not register");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let missing = home.root().join("missing-binding.json");
        assert_eq!(
            register(&home, Some(missing.as_os_str()))
                .await
                .expect_err("a missing controlled file is not an unbound MCP")
                .kind(),
            io::ErrorKind::NotFound
        );
        assert!(!home.root().exists());
    }
}
