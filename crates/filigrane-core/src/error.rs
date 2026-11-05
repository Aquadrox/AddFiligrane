use std::io;
use std::path::PathBuf;

/// Résultat des opérations de la bibliothèque.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Erreurs possibles. Les messages s'adressent directement à l'utilisateur final.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Le texte du filigrane est vide.")]
    EmptyText,

    #[error("Le texte du filigrane est trop long ({len} caractères, maximum {max}).")]
    TextTooLong { len: usize, max: usize },

    #[error("Le texte du filigrane compte trop de lignes ({lines}, maximum {max}).")]
    TooManyLines { lines: usize, max: usize },

    #[error("Caractères non pris en charge par la police : {}", format_chars(.0))]
    UnsupportedCharacters(Vec<char>),

    #[error("{0}")]
    InvalidParameter(String),

    #[error("Sélection de pages invalide : {0}")]
    InvalidPageSelection(String),

    #[error("Aucune page ne correspond à la sélection « {selection} » (le document compte {total} page(s)).")]
    NoPageSelected { selection: String, total: u32 },

    #[error("Le fichier de destination doit être différent du fichier source.")]
    SameInputOutput,

    #[error("Impossible de lire « {} » : {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },

    #[error("Impossible d'écrire « {} » : {source}", .path.display())]
    Write { path: PathBuf, source: io::Error },

    #[error("Le fichier n'est pas un PDF valide ou il est endommagé ({0}).")]
    InvalidPdf(String),

    #[error("Ce PDF est protégé par un mot de passe. Retirez la protection puis réessayez.")]
    PasswordProtected,

    #[error(
        "Ce PDF est chiffré : le mode « superposé » ne peut pas le modifier. \
         Utilisez le mode « incrusté »."
    )]
    EncryptedOverlay,

    #[error(
        "Le moteur de rendu PDFium est introuvable ({0}). \
         Vérifiez que pdfium.dll se trouve à côté de l'application."
    )]
    PdfiumUnavailable(String),

    #[error("Échec du rendu de la page {page} : {message}")]
    Render { page: u32, message: String },

    #[error("Opération annulée.")]
    Cancelled,
}

impl Error {
    pub(crate) fn read(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn write(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Write {
            path: path.into(),
            source,
        }
    }
}

fn format_chars(chars: &[char]) -> String {
    chars
        .iter()
        .map(|c| format!("« {c} » (U+{:04X})", u32::from(*c)))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_characters_lists_code_points() {
        let message = Error::UnsupportedCharacters(vec!['✓', '中']).to_string();
        assert!(message.contains("U+2713"), "{message}");
        assert!(message.contains("U+4E2D"), "{message}");
    }
}
