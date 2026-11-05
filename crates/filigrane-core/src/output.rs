//! Écriture atomique du fichier produit : un fichier temporaire voisin est renommé
//! seulement en cas de succès, si bien qu'un échec ou une annulation ne laisse jamais
//! de PDF tronqué à la place de la destination.
//!
//! Le fichier temporaire porte un nom aléatoire et est créé exclusivement
//! (`O_EXCL` / `CREATE_NEW`) : un lien symbolique préparé à l'avance n'est pas suivi.

use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::error::{Error, Result};

const BUFFER_SIZE: usize = 1 << 20;

#[derive(Debug)]
pub(crate) struct AtomicOutput {
    final_path: PathBuf,
    /// Le fichier temporaire est supprimé automatiquement s'il n'est pas validé.
    writer: CountingWriter<BufWriter<NamedTempFile>>,
}

impl AtomicOutput {
    pub fn create(final_path: &Path) -> Result<Self> {
        let file_name = final_path
            .file_name()
            .ok_or_else(|| Error::write(final_path, io::Error::other("nom de fichier manquant")))?;
        let directory = final_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut prefix = std::ffi::OsString::from(".");
        prefix.push(file_name);
        prefix.push(".");
        let temp = tempfile::Builder::new()
            .prefix(&prefix)
            .suffix(".filigrane-tmp")
            .tempfile_in(directory)
            .map_err(|e| Error::write(final_path, e))?;
        Ok(Self {
            final_path: final_path.to_owned(),
            writer: CountingWriter::new(BufWriter::with_capacity(BUFFER_SIZE, temp)),
        })
    }

    pub fn writer(&mut self) -> &mut CountingWriter<BufWriter<NamedTempFile>> {
        &mut self.writer
    }

    pub fn write_error(&self, source: io::Error) -> Error {
        Error::write(&self.final_path, source)
    }

    /// Vide les tampons, force l'écriture sur disque et remplace la destination.
    /// Renvoie la taille du fichier produit.
    pub fn commit(self) -> Result<u64> {
        let size = self.writer.position();
        let temp = self
            .writer
            .into_inner()
            .into_inner()
            .map_err(|e| Error::write(&self.final_path, e.into_error()))?;
        temp.as_file()
            .sync_all()
            .map_err(|e| Error::write(&self.final_path, e))?;
        temp.persist(&self.final_path)
            .map_err(|e| Error::write(&self.final_path, e.error))?;
        Ok(size)
    }
}

/// Compte les octets écrits, pour les décalages de la table de références croisées.
#[derive(Debug)]
pub(crate) struct CountingWriter<W> {
    inner: W,
    position: u64,
}

impl<W: Write> CountingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner, position: 0 }
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.position += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Refuse d'écrire par-dessus le fichier source.
pub(crate) fn ensure_distinct(input: &Path, output: &Path) -> Result<()> {
    let input = fs::canonicalize(input).map_err(|e| Error::read(input, e))?;
    let output_resolved = match fs::canonicalize(output) {
        Ok(path) => path,
        Err(_) => {
            let parent = output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let parent = fs::canonicalize(parent).map_err(|e| Error::write(output, e))?;
            parent.join(output.file_name().unwrap_or_default())
        }
    };
    let same = if cfg!(windows) {
        input.to_string_lossy().to_lowercase() == output_resolved.to_string_lossy().to_lowercase()
    } else {
        input == output_resolved
    };
    if same { Err(Error::SameInputOutput) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_replaces_destination() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.pdf");
        fs::write(&target, b"ancien").unwrap();
        let mut output = AtomicOutput::create(&target).unwrap();
        output.writer().write_all(b"nouveau").unwrap();
        assert_eq!(output.commit().unwrap(), 7);
        assert_eq!(fs::read(&target).unwrap(), b"nouveau");
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "le fichier temporaire doit disparaître"
        );
    }

    #[test]
    fn dropping_without_commit_leaves_no_trace() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.pdf");
        fs::write(&target, b"ancien").unwrap();
        {
            let mut output = AtomicOutput::create(&target).unwrap();
            output.writer().write_all(b"partiel").unwrap();
        }
        assert_eq!(fs::read(&target).unwrap(), b"ancien");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn temporary_file_does_not_follow_a_planted_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.pdf");
        #[cfg(unix)]
        {
            // L'ancien nom prévisible, piégé : il ne doit plus servir.
            let victim = dir.path().join("victime.txt");
            fs::write(&victim, b"intact").unwrap();
            std::os::unix::fs::symlink(&victim, dir.path().join(".out.pdf.filigrane-tmp")).unwrap();
            let mut output = AtomicOutput::create(&target).unwrap();
            output.writer().write_all(b"pdf").unwrap();
            output.commit().unwrap();
            assert_eq!(fs::read(&victim).unwrap(), b"intact");
        }
        let first = AtomicOutput::create(&target).unwrap();
        let second = AtomicOutput::create(&target).unwrap();
        assert_ne!(
            first.writer.inner.get_ref().path(),
            second.writer.inner.get_ref().path(),
            "chaque traitement a son propre fichier temporaire"
        );
    }

    #[test]
    fn same_input_and_output_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a.pdf");
        fs::write(&input, b"%PDF-").unwrap();
        let alias = dir.path().join(".").join("a.pdf");
        assert!(matches!(ensure_distinct(&input, &alias), Err(Error::SameInputOutput)));
        ensure_distinct(&input, &dir.path().join("b.pdf")).unwrap();
    }
}
