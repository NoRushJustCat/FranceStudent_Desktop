//! Persistance des runs : un fichier JSON par run, écriture atomique.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::model::RunState;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("stockage : {0}")]
    Io(#[from] std::io::Error),
    #[error("état de run invalide : {0}")]
    Json(#[from] serde_json::Error),
    #[error("run introuvable : {0}")]
    NotFound(String),
}

#[async_trait]
pub trait RunStore: Send + Sync {
    async fn save(&self, state: &RunState) -> Result<(), StoreError>;
    async fn load(&self, run_id: &str) -> Result<Option<RunState>, StoreError>;
    /// Runs non terminaux, du plus récent au plus ancien.
    async fn list_open(&self) -> Result<Vec<RunState>, StoreError>;
    async fn delete(&self, run_id: &str) -> Result<(), StoreError>;
}

/// Stockage disque : `<root>/<run_id>.json`.
#[derive(Debug, Clone)]
pub struct FsRunStore {
    root: PathBuf,
}

impl FsRunStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn file_name(run_id: &str) -> String {
        let safe: String = run_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{safe}.json")
    }

    fn path(&self, run_id: &str) -> PathBuf {
        self.root.join(Self::file_name(run_id))
    }
}

#[async_trait]
impl RunStore for FsRunStore {
    async fn save(&self, state: &RunState) -> Result<(), StoreError> {
        tokio::fs::create_dir_all(&self.root).await?;
        let path = self.path(&state.run_id);
        let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
        let text = serde_json::to_string_pretty(state)?;
        tokio::fs::write(&tmp, text).await?;
        // `rename` remplace sur Unix, pas sur Windows : on retire l'ancien fichier.
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            tokio::fs::remove_file(&path).await?;
        }
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    async fn load(&self, run_id: &str) -> Result<Option<RunState>, StoreError> {
        let path = self.path(run_id);
        match tokio::fs::read_to_string(&path).await {
            Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::Io(e)),
        }
    }

    async fn list_open(&self) -> Result<Vec<RunState>, StoreError> {
        let mut out = Vec::new();
        let mut dir = match tokio::fs::read_dir(&self.root).await {
            Ok(dir) => dir,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(StoreError::Io(e)),
        };
        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = tokio::fs::read_to_string(&path).await else {
                continue;
            };
            let Ok(state) = serde_json::from_str::<RunState>(&text) else {
                continue;
            };
            if !state.status.is_terminal() {
                out.push(state);
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    async fn delete(&self, run_id: &str) -> Result<(), StoreError> {
        match tokio::fs::remove_file(self.path(run_id)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::Io(e)),
        }
    }
}
