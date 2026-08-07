# `corvid_ui`

A retained widget tree that lays out in fixed point and names no device.

```rust
use corvid_fixed::I16F16;
use corvid_ui::{Length, Monospace, Rect, Scale, Tree, button, column, label, solve, style};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Intent {
    Play,
    Friends,
    Settings,
}

let menu = column()
    .gap(Length::rem(I16F16::ONE))
    .child(label("cradle").style(style::TITLE))
    .child(button("play", Intent::Play))
    .child(button("join a friend", Intent::Friends))
    .child(button("settings", Intent::Settings));

let mut tree = Tree::new();
tree.reconcile(menu);

let viewport = Rect::of(I16F16::from_f64(1280.0), I16F16::from_f64(720.0));
let painted = solve(&tree, &Monospace::DEFAULT, Scale::DEFAULT, viewport)?;

// Three buttons, and the focus can be on any of them.
assert_eq!(painted.focusable().count(), 3);
# Ok::<(), corvid_ui::TooLarge>(())
```

## The two halves

This crate is the half with no device in it: a tree, a layout, a focus, and
paint data. `corvid_ui_render` is the half that draws the paint data, and it is
the only one of the two that has heard of a GPU. The split is the same one
`corvid_mesh` and `corvid_mesh_render` make, and it is what lets a layout test
run in a process with no graphics stack in it — and what lets this crate build
for `thumbv7em-none-eabi`, a target with no operating system at all.

## Four devices, two primitives

```rust
use corvid_ui::{Compass, Position, Signal, Tree, button, column};

# #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
# enum Intent { Play }
# let viewport = corvid_ui::Rect::of(
#     corvid_fixed::I16F16::from_f64(640.0),
#     corvid_fixed::I16F16::from_f64(480.0),
# );
let mut tree = Tree::new();
tree.reconcile(column().child(button("play", Intent::Play)));
let painted = corvid_ui::solve(
    &tree,
    &corvid_ui::Monospace::DEFAULT,
    corvid_ui::Scale::DEFAULT,
    viewport,
)?;

// A stick, a d-pad or a Tab key.
tree.navigate(&painted, Compass::Next);
// A mouse, a finger, or an XR ray that `Painted::panel_to_layout` has already
// turned into one of these.
tree.point(&painted, painted.nodes[1].rect.centre());

let mut raised = Vec::new();
tree.signal(Signal::Activate, &mut raised);
assert_eq!(raised[0].intent, Intent::Play);
# Ok::<(), corvid_ui::TooLarge>(())
```

`Tree::navigate` is a spatial search over the resolved rectangles, scored
`along + 2 * across`: among the focusable nodes whose centre lies in the
half-plane the compass names, the one nearest by that score, where `along` is
the distance between the centres in the direction of travel and `across` is the
gap between the two rectangles perpendicular to it — zero while they overlap.
Weighting the perpendicular twice is what stops a stick-right from skipping a
column; measuring it as a gap rather than between centres is what stops a
stick-down out of a wide button skipping the narrow one under it.
`Compass::Next` and `Compass::Previous` are tree order instead, because that is
what a Tab key and a form mean, and conflating the two is why some menus skip a
button when the layout wraps.

`Painted::panel_to_layout` casts a world-space ray at the panel's quad and
answers a position in layout space. So the headset path and the mouse path are
the same code below the intersection, and there is no XR-specific branch
anywhere in this crate.

## A style is a value the program has before it runs

```rust
use corvid_color::Rgba8;
use corvid_fixed::I16F16;
use corvid_ui::{Align, Edges, Length, Style};

const CARD: Style = Style::new()
    .width(Length::rem(I16F16::from_f64(20.0)))
    .padding(Edges::all(Length::REM))
    .gap(Length::rem(I16F16::from_f64(0.5)))
    .corner(Length::rem(I16F16::from_f64(0.5)))
    .align(Align::Stretch)
    .background(Rgba8::hex(0x0F_17_2A_FF));

assert_eq!(CARD.align, Align::Stretch);
```

Every builder on `Style` is `const`, and every builder on `Element` is `const`
up to the point a `Vec` of children appears — `Element::on` is the one
exception, and it says why in its own documentation.

## Reconciling is what makes it retained

A game writes the whole tree out every frame. Each element carries a digest of
its own properties and a digest of its subtree; a subtree whose digest is
unchanged is kept whole, including its resolved layout and its focus.

```rust
use corvid_ui::{Rebuilt, Tree, column, label};

fn hud(score: u32) -> corvid_ui::Element<()> {
    column().child(label("score")).child(label(&score.to_string()))
}

let mut tree = Tree::new();
tree.reconcile(hud(0));

// An idle frame discovers it has nothing to do.
assert_eq!(tree.reconcile(hud(0)), Rebuilt::NOTHING);
// A frame that changed one leaf rewrites one leaf.
assert_eq!(tree.reconcile(hud(1)), Rebuilt { nodes: 1, subtrees: 0 });
```

The cost is that a rebuild hashes every node's properties even when nothing
changed. A three-hundred-node HUD spends a few microseconds a frame discovering
it need do nothing, which buys not having to make every widget a game writes
correct about its own dirty regions.

## Layout is deterministic, so a UI regression is a golden diff

Every length resolves to `I16F16` physical pixels and every division is exact:
three `Fraction` thirds of a hundred pixels fill exactly a hundred, with the
remainder on the last of them rather than spread between them, because
spreading it depends on the order a sum was evaluated in.

```rust
use corvid_fixed::{Factor16, I16F16};
use corvid_hash::digest;
use corvid_ui::{Length, Monospace, Rect, Scale, Tree, column, spacer};

let third = Length::Fraction(Factor16::from_f64(1.0 / 3.0));
let mut tree = Tree::<()>::new();
tree.reconcile(
    column()
        .axis(corvid_ui::Axis::Row)
        .width(Length::px(I16F16::from_f64(100.0)))
        .child(spacer().width(third))
        .child(spacer().width(third))
        .child(spacer().width(third)),
);

let viewport = Rect::of(I16F16::from_f64(100.0), I16F16::from_f64(10.0));
let painted = solve(&tree, &Monospace::DEFAULT, Scale::DEFAULT, viewport)?;
# use corvid_ui::solve;
let widths: Vec<f64> = painted.nodes[1..].iter().map(|n| n.rect.width.to_f64()).collect();
assert_eq!(widths.iter().sum::<f64>(), 100.0);

// The whole layout is one number, so a regression is a changed digest.
assert_eq!(digest(&painted), digest(&solve(&tree, &Monospace::DEFAULT, Scale::DEFAULT, viewport)?));
# Ok::<(), corvid_ui::TooLarge>(())
```

A resolved length that ran past what `I16F16` holds is a `TooLarge` naming the
node, rather than a silent saturation — the saturation would be a menu that is
subtly wrong on one machine and right on every other.

## Where the font comes from

`Metrics` is the three numbers a layout needs from a font: an advance, a line
height and an ascent. `Monospace` implements it with proportions that are exact
in `I16F16`, which is what a golden layout wants and what a game that has not
chosen a face yet can lay out against. A rasteriser implements the same trait
and the solver does not change.
