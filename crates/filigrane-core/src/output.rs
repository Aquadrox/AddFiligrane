//! Écriture atomique du fichier produit : un fichier temporaire voisin est renommé
//! seulement en cas de succès, si bien qu'un échec ou une annulation ne laisse jamais
//! de PDF tronqué à la place de la destination.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const BUFFER_SIZE: usize = 1 << 20;

#[derive(Debug)]
pub(crate) struct AtomicOutput {
    final_path: PathBuf,
    temp_path: PathBuf,
    writer: Option<CountingWriter<BufWriter<File>>>,
}

impl AtomicOutput {
    pub fn create(final_path: &Path) -> Result<Self> {
        let file_name = final_path
            .file_name()
            .ok_or_else(|| Error::write(final_path, io::Error::other("nom de fichier manquant")))?;
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(file_name);
        temp_name.push(".filigrane-tmp");
        let temp_path = final_path.with_file_name(temp_name);
        let file = File::create(&temp_path).map_err(|e| Error::write(final_path, e))?;
        Ok(Self {
            final_path: final_path.to_owned(),
            temp_path,
            writer: Some(CountingWriter::new(BufWriter::with_capacity(BUFFER_SIZE, file))),
        })
    }

    pub fn writer(&mut self) -> &mut CountingWriter<BufWriter<File>> {
        self.writer.as_mut().expect("sortie déjà validée")
    }

    pub fn write_error(&self, source: io::Error) -> Error {
        Error::write(&self.final_path, source)
    }

    /// Vide les tampons, force l'écriture sur disque et remplace la destination.
    /// Renvoie la taille du fichier produit.
    pub fn commit(mut self) -> Result<u64> {
        let writer = self.writer.take().expect("sortie déjà validée");
        let size = writer.position();
        let file = writer
            .into_inner()
            .into_inner()
            .map_err(|e| Error::write(&self.final_path, e.into_error()))?;
        file.sync_all().map_err(|e| Error::write(&self.final_path, e))?;
        drop(file);
        fs::rename(&self.temp_path, &self.final_path).map_err(|e| Error::write(&self.final_path, e))?;
        Ok(size)
    }
}

impl Drop for AtomicOutput {
    fn drop(&mut self) {
        if self.writer.take().is_some() {
            let _ = fs::remove_file(&self.temp_path);
        }
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
    fn same_input_and_output_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a.pdf");
        fs::write(&input, b"%PDF-").unwrap();
        let alias = dir.path().join(".").join("a.pdf");
        assert!(matches!(ensure_distinct(&input, &alias), Err(Error::SameInputOutput)));
        ensure_distinct(&input, &dir.path().join("b.pdf")).unwrap();
    }
}
