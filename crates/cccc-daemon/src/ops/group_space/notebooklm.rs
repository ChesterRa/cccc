use cccc_core::{HomeLayout, space_credentials};
use cccc_notebooklm::{Artifact, ArtifactGeneration, Client, Error, Notebook, QueryResult, Source};

use crate::dispatch::OpError;

pub(super) fn client(home: &HomeLayout) -> Result<Client, OpError> {
    let credential = space_credentials::resolve(home, "notebooklm")
        .map_err(OpError::io)?
        .ok_or_else(|| {
            OpError::new(
                "credential_missing",
                "NotebookLM auth storage state is not configured",
            )
        })?;
    Client::from_storage_state(&credential).map_err(map_error)
}

pub(super) fn health(home: &HomeLayout) -> Result<(), OpError> {
    run(home, Client::health_check)
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

pub(super) fn download_artifact(
    home: &HomeLayout,
    artifact: &Artifact,
) -> Result<Vec<u8>, OpError> {
    run(home, |client| client.download_artifact(artifact))
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
        Error::InvalidCredential(_) => "credential_invalid",
        Error::Authentication => "auth_expired",
        Error::Refused(_) => "provider_refused",
        Error::Transport(_) => "provider_transport_error",
        Error::Rpc { .. } => "provider_rpc_error",
        Error::SchemaDrift { .. } => "provider_schema_drift",
    };
    OpError::new(code, error.to_string())
}
