//! Comparing a capture directory against a directory of frozen bytes, and the
//! one sanctioned way to change one of them.

use std::{fs, io, path::Path};

use crate::mismatch::{Finding, How, Mismatch};

/// The environment variable that rewrites the goldens.
///
/// Set it to anything non-empty. Unset or empty compares.
pub const BLESS: &str = "CORVID_BLESS";

/// The extension a golden is written under.
///
/// A golden for the capture file `audio/42` is `audio/42.hex`. Anything under
/// the goldens directory without this extension is not a golden and is ignored,
/// so a README and the `frames/*.png` that
/// [`images_agree`](crate::images_agree) compares can sit beside them.
pub const EXTENSION: &str = "hex";

/// How many hex characters go on one line of a golden.
///
/// Thirty-two bytes. A capture is compared by a program and read by a person,
/// and a single four-thousand-character line is a diff that says one line
/// changed.
const WIDTH: usize = 64;

/// The sixteen characters a nibble is written as.
const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Compares every golden under `goldens` against the file it was recorded from
/// under `capture`.
///
/// # The goldens directory is the frozen set
///
/// A capture holds far more than anybody wants to freeze -- an audio frame per
/// displayed frame, and a picture of it wherever there was an adapter to draw
/// one. So the
/// authority for *what is frozen* is the goldens directory: every `.hex` file
/// under it names a capture file, by the same relative path with the extension
/// removed, and that file has to be there and has to hold those bytes. A capture
/// file with no golden beside it is not frozen and is not compared, which is how
/// four sampled frames out of two hundred is expressible without a list living
/// anywhere.
///
/// The consequence worth stating: **freezing one more file is creating one more
/// file.** `touch goldens/audio/17.hex` and then bless, and the capture's
/// `audio/17` is frozen from then on. Blessing never decides on its own what to
/// freeze, because "the capture grew a file" and "somebody meant to freeze it"
/// are not the same event.
///
/// A goldens directory with no goldens in it is [`Mismatch::Unfrozen`] and never
/// a pass. A comparison with nothing in it is the failure mode a golden test
/// exists to avoid.
/// # Blessing
///
/// With [`BLESS`] set to anything non-empty, every golden that no longer says
/// what the capture holds is rewritten from it, and **the call still fails**,
/// naming everything it rewrote. That is deliberate twice over: a blessing run
/// that went green would tell nobody what it changed, and a CI job with the
/// variable set by accident would go green forever. Run it again to see it pass.
///
/// A blessing run in a tree that already agrees rewrites nothing and returns
/// [`Ok`].
///
/// This is the only sanctioned way to change a golden. One edited by hand is a
/// golden that says what somebody expected rather than what the runtime
/// produced, which is the single thing a frozen capture is for. What blessing
/// does *not* do is decide whether the change was meant: a moved golden is
/// either the game's arithmetic changing or the capture format changing, both
/// are one command, and neither should be.
///
/// # Errors
///
/// [`Mismatch::Unfrozen`] if the goldens directory holds no goldens,
/// [`Mismatch::Unreadable`] if a directory could not be walked or a file
/// could not be read or written, [`Mismatch::Moved`] naming every golden that no
/// longer agrees, and [`Mismatch::Rewritten`] naming every golden a blessing run
/// rewrote.
pub fn matches_goldens(capture: &Path, goldens: &Path) -> Result<(), Mismatch> {
    let frozen = frozen(goldens)?;
    if frozen.is_empty() {
        return Err(Mismatch::Unfrozen {
            goldens: goldens.to_path_buf(),
        });
    }

    let blessing = std::env::var_os(BLESS).is_some_and(|value| !value.is_empty());
    let mut findings = Vec::new();
    for what in frozen {
        let golden = goldens.join(format!("{what}.{EXTENSION}"));
        let Some(how) = examine(&capture.join(&what), &golden)? else {
            continue;
        };
        if blessing && how.is_rewritable() {
            let captured = read(&capture.join(&what))?;
            write(&golden, hex(&captured).as_bytes())?;
        }
        findings.push(Finding { what, how });
    }

    if findings.is_empty() {
        return Ok(());
    }
    let goldens = goldens.to_path_buf();
    Err(if blessing {
        Mismatch::Rewritten { goldens, findings }
    } else {
        Mismatch::Moved { goldens, findings }
    })
}

/// How one golden and its capture file disagree, or [`None`] if they do not.
fn examine(captured: &Path, golden: &Path) -> Result<Option<How>, Mismatch> {
    let bytes = match fs::read(captured) {
        Ok(bytes) => bytes,
        // The one io failure that is a finding rather than an error: a golden
        // naming a file the capture does not hold is what a run that stopped
        // producing that file looks like, and it is exactly what the comparison
        // is for.
        Err(why) if why.kind() == io::ErrorKind::NotFound => return Ok(Some(How::Absent)),
        Err(why) => return Err(unreadable(captured, why)),
    };
    let text = fs::read_to_string(golden).map_err(|why| unreadable(golden, why))?;
    let Some(recorded) = unhex(&text) else {
        return Ok(Some(How::Malformed));
    };
    if recorded == bytes {
        return Ok(None);
    }
    Ok(Some(How::Moved {
        at: recorded
            .iter()
            .zip(&bytes)
            .position(|(one, two)| one != two)
            .unwrap_or_else(|| recorded.len().min(bytes.len())),
        recorded: recorded.len(),
        captured: bytes.len(),
    }))
}

/// Every golden under `goldens`, as a capture-relative path with the extension
/// removed, sorted.
///
/// Sorted because a report is read, and a report whose lines arrive in whatever
/// order the filesystem enumerated them is a report that looks different every
/// time nothing changed.
fn frozen(goldens: &Path) -> Result<Vec<String>, Mismatch> {
    let mut found = Vec::new();
    let mut stack = vec![goldens.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            // A goldens directory that is not there holds no goldens, which is
            // `Unfrozen` and not an unreadable path: it is what a game looks
            // like before it has frozen anything, and the message for that case
            // is the one that says what to do about it.
            Err(why) if why.kind() == io::ErrorKind::NotFound && directory == goldens => {
                return Ok(found);
            }
            Err(why) => return Err(unreadable(&directory, why)),
        };
        for entry in entries {
            let path = entry.map_err(|why| unreadable(&directory, why))?.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .extension()
                .is_none_or(|extension| extension != EXTENSION)
            {
                continue;
            }
            let relative = path.with_extension("");
            let Ok(relative) = relative.strip_prefix(goldens) else {
                continue;
            };
            // Lossy, and the loss is worth naming: a path this crate cannot
            // spell in UTF-8 is a path it cannot report either, and the capture
            // file it would be compared against is named after a `Tick`. A
            // golden with a name like that would fail to find its capture file
            // and report `Absent`, which is the answer a reader can act on.
            found.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    found.sort();
    Ok(found)
}

/// Reads a file, naming the path in whatever goes wrong.
fn read(path: &Path) -> Result<Vec<u8>, Mismatch> {
    fs::read(path).map_err(|why| unreadable(path, why))
}

/// Writes a file and the directories above it, naming the path in whatever goes
/// wrong.
fn write(path: &Path, bytes: &[u8]) -> Result<(), Mismatch> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|why| unreadable(parent, why))?;
    }
    fs::write(path, bytes).map_err(|why| unreadable(path, why))
}

/// The error a path and an `io::Error` make, since `io::Error` does not carry
/// the path it was about.
fn unreadable(path: &Path, why: io::Error) -> Mismatch {
    Mismatch::Unreadable {
        path: path.to_path_buf(),
        why,
    }
}

/// The bytes of a capture file, as the text a golden is written in.
///
/// Hex text rather than a binary blob, because a golden is reviewed as well as
/// compared: a diff over hex says which line moved, and a diff over a binary
/// file says "binary files differ".
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2 + bytes.len() / 32 + 1);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 && (index * 2).is_multiple_of(WIDTH) {
            text.push('\n');
        }
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text.push('\n');
    text
}

/// The bytes a golden's text was written from, or [`None`] if it is not whole
/// hex bytes.
///
/// Whitespace is not part of a golden: the line breaks [`hex`] wraps at are
/// there for whoever reads it.
#[must_use]
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}
