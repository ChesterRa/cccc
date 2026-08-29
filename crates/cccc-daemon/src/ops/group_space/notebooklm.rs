use cccc_core::{HomeLayout, space_credentials};
use cccc_notebooklm::{Artifact, ArtifactGeneration, Client, Error, Notebook, QueryResult, Source};
use std::path::Path;
use std::time::Duration;

use crate::dispatch::OpError;

pub(super) fn client(home: &HomeLayout) -> Result<Client, OpError> {
    let credential = space_credentials::resolve(home, "notebooklm")
        .map_err(OpError::io)?
        .ok_or_else(|| {
            OpError::new(
                "space_provider_not_configured",
                "NotebookLM auth storage state is not configured",
            )
        })?;
    Client::from_storage_state(&credential).map_err(map_error)
}

pub(super) fn health(home: &HomeLayout) -> Result<(), OpError> {
    run(home, Client::health_check)
}

pub(super) fn health_candidate(credential: &str) -> Result<(), OpError> {
    Client::from_storage_state(credential)
        .and_then(|client| client.health_check())
        .map_err(map_error)
}

pub(super) fn notebooks(home: &HomeLayout) -> Result<Vec<Notebook>, OpError> {
    run(home, Client::list_notebooks)
}

pub(super) fn create_notebook(home: &HomeLayout, title: &str) -> Result<Notebook, OpError> {
    run(home, |client| client.create_notebook(title))
}

pub(super) fn sources(home: &HomeLayout, notebook_id: &str) -> Result<Vec<Source>, OpError> {
    run(home, |client| client.list_sources(notebook_id))
}

pub(super) fn add_text(
    home: &HomeLayout,
    notebook_id: &str,
    title: &str,
    content: &str,
) -> Result<Source, OpError> {
    run(home, |client| {
        client.add_text_source(notebook_id, title, content)
    })
}

pub(super) fn add_url(
    home: &HomeLayout,
    notebook_id: &str,
    url: &str,
    title: Option<&str>,
) -> Result<Source, OpError> {
    run(home, |client| {
        client.add_url_source(notebook_id, url, title)
    })
}

pub(super) fn add_drive(
    home: &HomeLayout,
    notebook_id: &str,
    file_id: &str,
    title: &str,
    mime_type: &str,
) -> Result<Source, OpError> {
    run(home, |client| {
        client.add_drive_source(notebook_id, file_id, title, mime_type)
    })
}

pub(super) fn add_file(
    home: &HomeLayout,
    notebook_id: &str,
    file_path: &Path,
    title: Option<&str>,
) -> Result<Source, OpError> {
    run(home, |client| {
        client.add_file_source(notebook_id, file_path, title)
    })
}

pub(super) fn query(
    home: &HomeLayout,
    notebook_id: &str,
    question: &str,
    source_ids: Option<&[String]>,
) -> Result<QueryResult, OpError> {
    run(home, |client| {
        client.query_scoped(notebook_id, question, source_ids)
    })
}

pub(super) fn delete_source(
    home: &HomeLayout,
    notebook_id: &str,
    source_id: &str,
) -> Result<(), OpError> {
    run(home, |client| client.delete_source(notebook_id, source_id))
}

pub(super) fn refresh_source(
    home: &HomeLayout,
    notebook_id: &str,
    source_id: &str,
) -> Result<(), OpError> {
    run(home, |client| client.refresh_source(notebook_id, source_id))
}

pub(super) fn rename_source(
    home: &HomeLayout,
    notebook_id: &str,
    source_id: &str,
    title: &str,
) -> Result<(), OpError> {
    run(home, |client| {
        client.rename_source(notebook_id, source_id, title)
    })
}

pub(super) fn artifacts(home: &HomeLayout, notebook_id: &str) -> Result<Vec<Artifact>, OpError> {
    run(home, |client| client.list_artifacts(notebook_id))
}

pub(super) fn generate_artifact(
    home: &HomeLayout,
    notebook_id: &str,
    kind: &str,
    language: &str,
    instructions: Option<&str>,
    source_ids: Option<&[String]>,
) -> Result<ArtifactGeneration, OpError> {
    run(home, |client| {
        client.generate_artifact(notebook_id, kind, language, instructions, source_ids)
    })
}

pub(super) fn wait_artifact(
    home: &HomeLayout,
    notebook_id: &str,
    artifact_id: &str,
    timeout: Duration,
    initial_interval: Duration,
    max_interval: Duration,
) -> Result<Artifact, OpError> {
    run(home, |client| {
        client.wait_for_artifact(
            notebook_id,
            artifact_id,
            timeout,
            initial_interval,
            max_interval,
        )
    })
}

pub(super) fn download_artifact(
    home: &HomeLayout,
    artifact: &Artifact,
    output_format: Option<&str>,
) -> Result<Vec<u8>, OpError> {
    run(home, |client| {
        client.download_artifact(artifact, output_format)
    })
}

fn run<T>(
    home: &HomeLayout,
    operation: impl FnOnce(&Client) -> cccc_notebooklm::Result<T>,
) -> Result<T, OpError> {
    let client = client(home)?;
    let result = operation(&client).map_err(map_error);
    persist_rotated_credentials(home, &client);
    result
}

fn persist_rotated_credentials(home: &HomeLayout, client: &Client) {
    let source = space_credentials::status(home, "notebooklm")
        .ok()
        .and_then(|status| status["source"].as_str().map(str::to_owned));
    if source.as_deref() != Some("store") {
        return;
    }
    let result = client.storage_state().and_then(|storage| {
        serde_json::to_string(&storage).map_err(|error| Error::InvalidCredential(error.to_string()))
    });
    match result {
        Ok(raw) => {
            if space_credentials::resolve(home, "notebooklm")
                .ok()
                .flatten()
                .as_deref()
                == Some(raw.as_str())
            {
                return;
            }
            if let Err(error) = space_credentials::update(home, "notebooklm", &raw) {
                tracing::warn!(%error, "failed to persist rotated NotebookLM credentials");
            }
        }
        Err(error) => {
            tracing::warn!(%error, "failed to snapshot rotated NotebookLM credentials");
        }
    }
}

fn map_error(error: Error) -> OpError {
    let code = match error {
        Error::InvalidCredential(_) | Error::Authentication => "space_provider_auth_invalid",
        Error::RateLimited(_) => "space_provider_rate_limited",
        Error::Timeout(_) => "space_provider_timeout",
        Error::Unresolved(_) => "space_provider_outcome_unresolved",
        Error::Refused(_) | Error::Transport(_) | Error::Rpc { .. } => {
            "space_provider_upstream_error"
        }
        Error::SchemaDrift { .. } => "space_provider_compat_mismatch",
    };
    OpError::new(code, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::map_error;
    use cccc_notebooklm::Error;

    #[test]
    fn maps_native_failures_to_shared_provider_error_codes() {
        assert_eq!(
            map_error(Error::Authentication).code,
            "space_provider_auth_invalid"
        );
        assert_eq!(
            map_error(Error::RateLimited("quota".into())).code,
            "space_provider_rate_limited"
        );
        assert_eq!(
            map_error(Error::Timeout("wait".into())).code,
            "space_provider_timeout"
        );
        assert_eq!(
            map_error(Error::Unresolved("remote outcome is unknown".into())).code,
            "space_provider_outcome_unresolved"
        );
        assert_eq!(
            map_error(Error::SchemaDrift {
                context: "test",
                message: "shape".into(),
            })
            .code,
            "space_provider_compat_mismatch"
        );
    }
}
