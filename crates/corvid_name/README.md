# `corvid_name`

A short name that lives inside the value that holds it, encodes as its text,
and refuses anything that does not fit.

## Technical details

A name is `[u8; N]` with the unused tail left zero, and nothing else. There is
no pointer, no length field and no allocation, so a name sits inside an enum or
a trait argument at its own width and copies when the value around it copies.
The crate needs no allocator, which is what lets a `no_std` crate with none
still hold names.

NUL is the one byte a name may not contain. That is what makes the padding
unambiguous, and it is also what makes the derived `Ord` the string order: zero
sorts below every byte a name is allowed to hold, so comparing the arrays and
comparing the text give the same answer. `Eq`, `Ord` and `Hash` are all derived
over the array, so the three agree with each other and the capacity is part of
the digest -- a name type's width is a wire-format decision rather than an
implementation one.

The encoding is the text, not the array. A name written into a save or a
capture reads as `"terminus"` rather than as thirty-two numbers, which also
means a capacity can grow later without invalidating anything already written.
Reading one back re-checks the bound, because a file is not a `&str` this
program built and a name that no longer fits has to be refused rather than
quietly cut.

[`bounded_name!`] is the interface: it declares a public newtype over a
[`Name`] of a stated capacity, with the constructor, the accessors, `Display`
and `Debug`. Writing those out per name type would be one bug waiting for one
of the copies to be fixed. The encoding sits behind a `serde` feature read in
the **calling** crate, since a `cfg` resolves where it expands and a macro
crate cannot take a dependency on its caller's behalf.

```rust
corvid_name::bounded_name! {
    /// The line a friends list shows.
    PresenceText, 64
}

let line = PresenceText::new("in the lobby").expect("twelve bytes fit in sixty-four");
assert_eq!(line.as_str(), "in the lobby");
```

## Scope

Names bounded at a capacity the type states. That is the whole of it.

This is not a small-string optimization. A [`SmallString`] spills to the heap
when it outgrows its inline buffer, which is the right trade for a general
string and the wrong one here: what a name buys is that every peer agrees on
the bound, so a value too long is an error where it was built rather than a
different length on a different machine. Spilling would make the bound
advisory, take away `Copy`, and put an allocation back in a type chosen for not
having one.

It is also not a string type. There is no concatenation, no formatting, no
slicing and no `Deref<Target = str>` -- [`as_str`] hands over the text and
`core::str` is where the operations live. A name is an identifier that happens
to be readable.

[`bounded_name!`]: macro.bounded_name.html
[`Name`]: struct.Name.html
[`as_str`]: struct.Name.html#method.as_str
[`SmallString`]: https://docs.rs/smallstr
