//! The trait's own contract: the three findings, the sources that are not
//! `Memory`, and what an implementation gets for free — including the write it
//! is entitled to refuse.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

use std::thread;

use corvid_files::{Malformed, Memory, Missing, ReadOnly, Source};

/// The two failures are different findings and the types keep them apart.
#[test]
fn absent_and_unreadable_are_not_the_same_failure() {
    let missing = Missing::new("level/court.bin");
    let malformed = Malformed::from(missing);
    assert_eq!(malformed.path.as_deref(), Some("level/court.bin"));
    assert_ne!(
        Malformed::at("level/court.bin", "the header is the wrong version"),
        malformed,
        "a file that is absent and a file that will not parse are two findings",
    );
}

/// A decoder that was handed bytes and nothing else says so, and whoever read
/// them fills the path in afterwards.
///
/// This is the arrangement that lets `Asset::decode` and `Level::load` raise one
/// type: the decoder knows what was wrong, the store knows where it came from,
/// and neither has to know the other's half.
#[test]
fn a_path_can_be_named_after_the_fact() {
    let from_a_decoder = Malformed::new("the header is the wrong version");
    assert_eq!(from_a_decoder.path, None);
    assert_eq!(
        from_a_decoder.in_file("level/court.bin"),
        Malformed::at("level/court.bin", "the header is the wrong version"),
    );
}

/// A decoder with only a sentence to offer converts it with `?`, which is the
/// only reason the two `From` impls exist.
#[test]
fn a_sentence_on_its_own_is_already_a_failure() {
    let borrowed: Malformed = "the header is the wrong version".into();
    let owned: Malformed = String::from("the header is the wrong version").into();
    assert_eq!(borrowed, owned);
    assert_eq!(borrowed, Malformed::new("the header is the wrong version"));
    assert_eq!(
        borrowed.path, None,
        "a decoder handed bytes and nothing else cannot know the path"
    );
}

/// What each failure says when something prints it, which is what a server log
/// and a crash report show a human.
#[test]
fn each_failure_says_which_finding_it_is_when_printed() {
    assert_eq!(
        Missing::new("level/court.bin").to_string(),
        "nothing to read at level/court.bin"
    );
    assert_eq!(
        Malformed::new("the header is the wrong version").to_string(),
        "malformed asset: the header is the wrong version"
    );
    assert_eq!(
        Malformed::at("level/court.bin", "the header is the wrong version").to_string(),
        "level/court.bin could not be read: the header is the wrong version"
    );
    assert_eq!(
        Malformed::from(Missing::new("level/court.bin")).to_string(),
        "level/court.bin could not be read: it is not there",
        "the widening keeps the path and says what it says",
    );
}

/// A game whose levels are constants still has to hand `load` a source, and
/// `()` is what a caller with nothing to hand over hands over.
///
/// Every read fails, which is the honest answer for a source with no files in
/// it; the listing is empty rather than an error, because "there is nothing
/// here" is an answer and not a refusal to be asked.
#[test]
fn the_unit_source_holds_nothing_and_says_so_rather_than_panicking() {
    assert_eq!(
        Source::read(&(), "level/court.bin").expect_err("() has no files"),
        Missing::new("level/court.bin"),
        "and it names the path it was asked for",
    );
    assert_eq!(
        Source::list(&()).expect("an empty source is still askable"),
        Vec::<String>::new()
    );
    assert!(!Source::exists(&(), "level/court.bin"));
    assert_eq!(
        Source::write(&mut (), "level/court.bin", &[1]).expect_err("() takes no writes either"),
        ReadOnly::new("level/court.bin"),
        "and the refusal names the path the write was aimed at",
    );
}

/// A source that overrides nothing refuses every write, and says which path it
/// refused rather than answering a bare unit.
///
/// The default is what most implementations will keep — a directory mounted for
/// reading, an archive, a constant compiled in — so it is the behaviour worth
/// pinning rather than the override.
#[test]
fn a_source_that_overrides_nothing_refuses_the_write_and_names_the_path() {
    struct ReadsOnly;

    impl Source for ReadsOnly {
        fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
            Err(Missing::new(path))
        }

        fn list(&self) -> Result<Vec<String>, Missing> {
            Ok(Vec::new())
        }
    }

    assert_eq!(
        ReadsOnly
            .write("level/court.bin", &[1, 2, 3])
            .expect_err("the default refuses"),
        ReadOnly::new("level/court.bin"),
    );
}

/// A shared borrow of a writable source cannot write through to it.
///
/// This is the property the `&mut self` receiver exists for, and it is worth a
/// test rather than a sentence because it is the one place the blanket impl on
/// `&T` deliberately does *not* forward: `&mut &Memory` is a mutable borrow of
/// the reference, not of the map, so there is no `&mut Memory` to reach. A
/// `Level::load` handed a `&dyn Source` is on the wrong side of exactly this,
/// which is what keeps a load from writing during its own load.
#[test]
fn a_shared_borrow_cannot_write_through_to_the_source_behind_it() {
    let mut files = Memory::new();
    files
        .write("level/court.bin", &[1])
        .expect("Memory takes writes");

    let mut borrowed: &Memory = &files;
    assert_eq!(
        borrowed
            .write("level/court.bin", &[9, 9, 9])
            .expect_err("a shared borrow inherits the refusing default"),
        ReadOnly::new("level/court.bin"),
    );

    assert_eq!(
        files
            .read("level/court.bin")
            .expect("still the first write"),
        [1],
        "and the refusal was a refusal, not a write that went somewhere else",
    );
}

/// What a refused write says when something prints it, alongside the two
/// findings on the reading side.
#[test]
fn a_refused_write_says_which_finding_it_is_when_printed() {
    assert_eq!(
        ReadOnly::new("level/court.bin").to_string(),
        "nothing can be written to level/court.bin"
    );
}

/// A borrow of a source satisfies an `S: Source` bound, and that — not the
/// coercion to `&dyn Source`, which needs no help — is what the impl on `&T`
/// is for.
#[test]
fn a_borrow_of_a_source_is_itself_a_source() {
    // By value, not by reference: the whole question is whether a borrow can
    // *be* the `S`, and a `&S` parameter would answer a different one.
    fn through_the_bound<S: Source>(source: S) -> (Vec<u8>, Vec<String>, bool) {
        (
            source.read("level/court.bin").expect("just inserted"),
            source.list().expect("a source with an entry"),
            source.exists("level/absent.bin"),
        )
    }

    let mut files = Memory::new();
    files.insert("level/court.bin", vec![7, 8]);
    let expected = (vec![7, 8], vec![String::from("level/court.bin")], false);

    assert_eq!(through_the_bound(&files), expected, "a plain borrow");

    // The `?Sized` half: a `&dyn Source` handed down from a `load` further up
    // is itself an `S`, so it can be passed on without being re-boxed.
    let erased: &dyn Source = &files;
    assert_eq!(
        through_the_bound(erased),
        expected,
        "a borrowed trait object"
    );
}

/// The borrow forwards `exists` as well, rather than letting the default stand
/// in for the override the source already has.
///
/// A `&T` that inherited the default would quietly turn a source's cheap name
/// lookup back into a read of the whole file, and nothing about the answer would
/// show it. The source here answers the two questions differently on purpose, so
/// the answer does show it.
#[test]
fn a_borrow_forwards_exists_rather_than_falling_back_to_the_default() {
    struct AnswersWithoutReading;

    impl Source for AnswersWithoutReading {
        fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
            Err(Missing::new(path))
        }

        fn list(&self) -> Result<Vec<String>, Missing> {
            Ok(Vec::new())
        }

        fn exists(&self, _path: &str) -> bool {
            true
        }
    }

    fn through_the_bound<S: Source>(source: S) -> bool {
        source.exists("level/court.bin")
    }

    assert!(
        through_the_bound(&AnswersWithoutReading),
        "the default would have read the file, failed, and answered false"
    );
}

/// A source that writes only the two required methods still answers `exists`,
/// by reading the file and throwing the bytes away.
///
/// Wasteful and correct, which is why it is a default rather than a
/// requirement — and why both of the sources shipped here override it.
#[test]
fn the_default_exists_reads_the_file_and_drops_the_bytes() {
    struct TwoMethodsOnly(Memory);

    impl Source for TwoMethodsOnly {
        fn read(&self, path: &str) -> Result<Vec<u8>, Missing> {
            self.0.read(path)
        }

        fn list(&self) -> Result<Vec<String>, Missing> {
            self.0.list()
        }
    }

    let mut files = Memory::new();
    files.insert("level/court.bin", vec![1, 2, 3]);
    let source = TwoMethodsOnly(files);

    assert!(source.exists("level/court.bin"));
    assert!(!source.exists("level/absent.bin"));
}

/// The trait is `Send + Sync` because a level is read off the tick, on a thread
/// of its own, and the source is shared with it.
///
/// The bound is the claim the whole synchronous design rests on, so this hands
/// a source to another thread rather than merely asserting that it could.
#[test]
fn a_source_can_be_read_from_a_loader_thread() {
    let mut files = Memory::new();
    files.insert("level/court.bin", vec![9]);
    let source: &dyn Source = &files;

    let bytes = thread::scope(|loader| {
        loader
            .spawn(|| source.read("level/court.bin"))
            .join()
            .expect("the loader thread did not panic")
    });
    assert_eq!(bytes.expect("just inserted"), [9]);
}
