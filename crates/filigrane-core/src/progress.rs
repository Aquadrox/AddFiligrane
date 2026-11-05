use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};

/// Étape en cours d'un traitement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Ouverture et analyse du document.
    Opening,
    /// Traitement des pages (`done` sur `total`).
    Pages { done: u32, total: u32 },
    /// Recopie du document d'origine (mode superposé), en octets.
    Copying { done: u64, total: u64 },
    /// Écriture des dernières structures et enregistrement du fichier.
    Finalizing,
}

impl Progress {
    /// Avancement entre 0 et 1, ou `None` si l'étape n'est pas mesurable.
    pub fn fraction(&self) -> Option<f32> {
        let ratio = |done: f64, total: f64| (total > 0.0).then(|| (done / total).clamp(0.0, 1.0) as f32);
        match *self {
            Self::Opening | Self::Finalizing => None,
            Self::Pages { done, total } => ratio(f64::from(done), f64::from(total)),
            Self::Copying { done, total } => ratio(done as f64, total as f64),
        }
    }

    /// Libellé court destiné à l'utilisateur.
    pub fn label(&self) -> String {
        match *self {
            Self::Opening => "Analyse du document…".into(),
            Self::Pages { done, total } => format!("Page {done} / {total}"),
            Self::Copying { done, total } => format!(
                "Recopie du document : {} / {} Mo",
                done / 1_000_000,
                total.div_ceil(1_000_000)
            ),
            Self::Finalizing => "Enregistrement…".into(),
        }
    }
}

/// Reçoit les étapes d'avancement. Appelé depuis le fil de traitement.
pub trait ProgressSink: Sync {
    fn report(&self, progress: Progress);
}

impl<F: Fn(Progress) + Sync> ProgressSink for F {
    fn report(&self, progress: Progress) {
        self(progress)
    }
}

/// Permet d'interrompre un traitement depuis un autre fil.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// `Err(Error::Cancelled)` si l'annulation a été demandée.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions_are_clamped() {
        assert_eq!(Progress::Pages { done: 5, total: 10 }.fraction(), Some(0.5));
        assert_eq!(Progress::Copying { done: 20, total: 10 }.fraction(), Some(1.0));
        assert_eq!(Progress::Pages { done: 0, total: 0 }.fraction(), None);
        assert_eq!(Progress::Opening.fraction(), None);
    }

    #[test]
    fn cancellation_is_shared_between_clones() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(token.check().is_ok());
        clone.cancel();
        assert!(matches!(token.check(), Err(Error::Cancelled)));
    }
}
