use super::*;

#[test]
fn confirmation_pty_preserves_the_configured_settings_sources() {
    if isolated("confirmation::options::confirmation_pty_preserves_the_configured_settings_sources")
    {
        return;
    }
    for arguments in [
        vec![
            "--bare",
            "--strict-mcp-config",
            "--setting-sources",
            "project,local",
        ],
        vec![
            "--setting-sources=user,local",
            "--strict-mcp-config",
            "--bare",
        ],
    ] {
        let mut f = Fixture::new("disclaimer");
        std::fs::remove_file(f.receipt_path()).expect("remove saved fixture receipt");
        f.actor
            .command
            .extend(arguments.iter().map(|value| value.to_string()));
        f.group.actors = vec![f.actor.clone()];
        cccc_core::GroupStore::new(f.home.clone())
            .expect("open fixture group store")
            .save(&f.group)
            .expect("save configured fixture actor");
        start_prompt(&f);
        let prompt: Value = cccc_core::fs::read_json(&f.config.join("prompt_launch.json"))
            .expect("read confirmation launch");
        let argv = prompt["argv"].as_array().expect("confirmation arguments");
        assert!(argv.windows(arguments.len()).any(|window| {
            window
                .iter()
                .zip(&arguments)
                .all(|(actual, expected)| actual == expected)
        }));
        assert!(!f.config.join("confirmation_inputs").exists());
        assert!(!headless::running(&f.group.group_id, &f.actor.id));
    }
}
