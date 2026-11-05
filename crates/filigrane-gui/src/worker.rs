//! Exécution d'un traitement en arrière-plan, sans bloquer l'interface.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use filigrane_core::{CancelToken, Job, Progress, Report};

/// Issue d'un traitement terminé.
#[derive(Debug)]
pub struct Outcome {
    pub output: PathBuf,
    pub result: filigrane_core::Result<Report>,
    pub elapsed: Duration,
}

/// Traitement en cours.
#[derive(Debug)]
pub struct Worker {
    cancel: CancelToken,
    progress: Arc<Mutex<Progress>>,
    handle: Option<JoinHandle<Outcome>>,
    output: PathBuf,
    started: Instant,
}

impl Worker {
    /// Lance le traitement ; `on_update` est appelé depuis le fil de travail à chaque
    /// étape (typiquement pour redemander un affichage).
    pub fn spawn(job: Job, on_update: impl Fn() + Send + Sync + 'static) -> Self {
        let cancel = CancelToken::new();
        let progress = Arc::new(Mutex::new(Progress::Opening));
        let output = job.output.clone();
        let handle = {
            let cancel = cancel.clone();
            let progress = Arc::clone(&progress);
            std::thread::Builder::new()
                .name("filigrane".into())
                .spawn(move || {
                    let started = Instant::now();
                    let report = |step: Progress| {
                        *progress.lock().unwrap_or_else(|e| e.into_inner()) = step;
                        on_update();
                    };
                    let result = filigrane_core::apply(&job, &report, &cancel);
                    on_update();
                    Outcome {
                        output: job.output,
                        result,
                        elapsed: started.elapsed(),
                    }
                })
                .expect("création du fil de traitement")
        };
        Self {
            cancel,
            progress,
            handle: Some(handle),
            output,
            started: Instant::now(),
        }
    }

    pub fn progress(&self) -> Progress {
        *self.progress.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelling(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Récupère le résultat si le traitement est terminé.
    pub fn try_finish(&mut self) -> Option<Outcome> {
        if !self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        let handle = self.handle.take()?;
        Some(handle.join().unwrap_or_else(|panic| {
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("erreur inconnue");
            Outcome {
                output: self.output.clone(),
                result: Err(filigrane_core::Error::InvalidPdf(format!("erreur interne : {message}"))),
                elapsed: self.started.elapsed(),
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn reports_the_outcome_and_notifies_the_interface() {
        let updates = Arc::new(AtomicUsize::new(0));
        let job = Job {
            input: PathBuf::from("introuvable.pdf"),
            output: PathBuf::from("sortie.pdf"),
            spec: filigrane_core::WatermarkSpec {
                text: "X".into(),
                ..Default::default()
            },
            mode: filigrane_core::Mode::default(),
        };
        let mut worker = Worker::spawn(job, {
            let updates = Arc::clone(&updates);
            move || {
                updates.fetch_add(1, Ordering::Relaxed);
            }
        });
        let outcome = loop {
            if let Some(outcome) = worker.try_finish() {
                break outcome;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(outcome.output, PathBuf::from("sortie.pdf"));
        assert!(matches!(outcome.result, Err(filigrane_core::Error::Read { .. })));
        assert!(updates.load(Ordering::Relaxed) >= 1);
        assert!(worker.try_finish().is_none(), "le résultat n'est rendu qu'une fois");
    }
}
