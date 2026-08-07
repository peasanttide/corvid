//! What a rebuild costs, and what it keeps.
//!
//! Every assertion here is about the same claim: a game writes its whole tree
//! out every frame, and the frames where nothing changed cost nothing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_hash::digest;

use corvid_ui::{Element, Key, NodeId, Rebuilt, Tree, column, label};
/// What the trees below raise. Three variants, because a menu has three
/// buttons and a fourth would say nothing the third does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Intent {
    Play,
    Friends,
    Settings,
}

/// A score panel, so a rebuild has one leaf that changes and one that does
/// not.
fn hud(score: u32) -> Element<Intent> {
    column().child(label("score")).child(label(match score {
        0 => "nil",
        1 => "one",
        _ => "many",
    }))
}

#[test]
fn a_node_id_is_four_bytes() {
    assert_eq!(size_of::<NodeId>(), 4);
    assert_eq!(NodeId::NONE.0, u32::MAX);
    assert!(NodeId::NONE.is_none());
    assert_eq!(NodeId::NONE.index(), None);
    assert_eq!(NodeId(7).index(), Some(7));
}

#[test]
fn the_same_tree_twice_costs_nothing() {
    let mut tree = Tree::new();
    let first = tree.reconcile(hud(0));
    assert_eq!(first.nodes, 3);
    assert_eq!(first.subtrees, 3);

    assert_eq!(tree.reconcile(hud(0)), Rebuilt::NOTHING);
    assert!(tree.reconcile(hud(0)).is_nothing());
    assert_eq!(tree.len(), 3);
}

#[test]
fn one_changed_leaf_rewrites_one_leaf() {
    let mut tree = Tree::new();
    tree.reconcile(hud(0));
    let before: Vec<NodeId> = tree.preorder().collect();

    assert_eq!(
        tree.reconcile(hud(1)),
        Rebuilt {
            nodes: 1,
            subtrees: 0
        }
    );
    // The ancestors' subtree digests moved; their identities did not.
    assert_eq!(tree.preorder().collect::<Vec<_>>(), before);
}

#[test]
fn children_are_in_declaration_order_and_stop() {
    let mut tree = Tree::<Intent>::new();
    tree.reconcile(
        column()
            .child(label("one"))
            .child(label("two"))
            .child(label("three")),
    );
    let children: Vec<NodeId> = tree.children(tree.root()).collect();
    assert_eq!(children.len(), 3);
    assert_eq!(
        tree.node(children[2]).unwrap().next_sibling,
        NodeId::NONE,
        "the last child's link is the terminator, not a slot"
    );
    for (at, child) in children.iter().enumerate() {
        assert_eq!(
            tree.node(*child).unwrap().key,
            Key::Index(u32::try_from(at).unwrap())
        );
        assert_eq!(tree.node(*child).unwrap().parent, tree.root());
    }
}

/// Rows keyed on a name, so a removal above them is a removal and not a
/// renumbering.
fn named(rows: &[(u64, &str)]) -> Element<Intent> {
    column().children(
        rows.iter()
            .map(|(key, text)| label(text).keyed(*key).focusable(true)),
    )
}

/// The same rows with no names, so their keys are their positions.
fn positional(rows: &[(u64, &str)]) -> Element<Intent> {
    column().children(rows.iter().map(|(_, text)| label(text).focusable(true)))
}

/// What one node says, for telling apart two nodes with the same id.
fn text_of(tree: &Tree<Intent>, node: NodeId) -> String {
    tree.node(node)
        .map(|node| match node.kind {
            corvid_ui::Kind::Label { text, .. } => text.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

#[test]
fn a_named_row_keeps_its_node_when_the_row_above_it_goes() {
    let all = [(1, "one"), (2, "two"), (3, "three")];
    let mut tree = Tree::new();
    tree.reconcile(named(&all));
    let third = tree.children(tree.root()).nth(2).unwrap();
    assert_eq!(text_of(&tree, third), "three");

    tree.reconcile(named(&all[1..]));
    let moved = tree.children(tree.root()).nth(1).unwrap();
    assert_eq!(moved, third, "a named row is the same node it was");
    assert_eq!(text_of(&tree, third), "three");
}

#[test]
fn a_positional_row_does_not() {
    let all = [(1, "one"), (2, "two"), (3, "three")];
    let mut tree = Tree::new();
    tree.reconcile(positional(&all));
    let third = tree.children(tree.root()).nth(2).unwrap();
    assert_eq!(text_of(&tree, third), "three");

    tree.reconcile(positional(&all[1..]));
    assert_eq!(tree.children(tree.root()).count(), 2);
    assert!(
        tree.node(third).is_none(),
        "the third slot had no third row to hold and was released"
    );
    let now = tree.children(tree.root()).nth(1).unwrap();
    assert_eq!(text_of(&tree, now), "three");
    assert_ne!(now, third, "the row moved to the node the second row had");
}

/// Ten thousand nodes in one chain. Every walk in this crate is an explicit
/// stack, so this is a slow frame rather than a blown stack.
fn chain(depth: usize) -> Element<Intent> {
    let mut element = label("leaf");
    for _ in 1..depth {
        element = column().child(element);
    }
    element
}

#[test]
fn a_tree_ten_thousand_deep_does_not_recurse() {
    let mut tree = Tree::new();
    let built = tree.reconcile(chain(10_000));
    assert_eq!(built.nodes, 10_000);
    assert_eq!(tree.len(), 10_000);
    assert_eq!(tree.preorder().count(), 10_000);

    // And the frame after it, which discards ten thousand elements it did not
    // need — the path that recursion would have blown rather than the one that
    // built them.
    assert_eq!(tree.reconcile(chain(10_000)), Rebuilt::NOTHING);
}

#[test]
fn a_whole_ui_is_one_digest() {
    let mut one = Tree::new();
    let mut two = Tree::new();
    one.reconcile(hud(0));
    two.reconcile(hud(0));
    assert_eq!(digest(&one), digest(&two));

    two.reconcile(hud(1));
    assert_ne!(digest(&one), digest(&two));

    // And a tree that arrived at the same shape by a different route hashes
    // the same, because the walk is hashed rather than the arena.
    let mut three = Tree::new();
    three.reconcile(hud(1));
    three.reconcile(hud(0));
    three.reconcile(hud(1));
    assert_eq!(digest(&two), digest(&three));
}

#[test]
fn an_element_knows_its_own_subtree() {
    let menu = column()
        .child(label("cradle"))
        .child(corvid_ui::button("play", Intent::Play))
        .child(corvid_ui::button("join a friend", Intent::Friends))
        .child(corvid_ui::button("settings", Intent::Settings));
    assert_eq!(menu.count(), 8);

    let same = column()
        .child(label("cradle"))
        .child(corvid_ui::button("play", Intent::Play))
        .child(corvid_ui::button("join a friend", Intent::Friends))
        .child(corvid_ui::button("settings", Intent::Settings));
    assert_eq!(menu.subtree_digest(), same.subtree_digest());
    assert_eq!(digest(&menu), digest(&same));

    let different: Element<Intent> = column().child(label("cradle"));
    assert_ne!(menu.subtree_digest(), different.subtree_digest());
}
