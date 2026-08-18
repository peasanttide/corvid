//! Colouring the triangles so a tick can be threaded.

use alloc::vec;
use alloc::vec::Vec;

use crate::cords::NavTriRef;
use crate::seam::NavTriEdge;
use crate::tri::NavTri;

/// The most colours a surface can need.
///
/// A triangle has three edges, so it has at most three neighbours, so a greedy
/// pass that gives each triangle the lowest colour none of its neighbours has
/// never reaches a fifth. This is a fact about triangles rather than a budget,
/// which is why the classes can be a fixed array of starts.
pub const MAX_COLOURS: usize = 4;

/// The colour of every triangle, and the triangles of every colour.
///
/// **No two triangles that share an edge share a colour.** That is the whole
/// promise, and what it is for is threading: a caller steps one colour class at
/// a time and no two threads ever touch adjacent triangles, so the crowd and
/// the diffusion parallelise with no lock and no atomics -- a triangle's
/// neighbours are exactly what a step of either reads and writes, and they are
/// exactly what a class does not contain.
///
/// The colouring is **greedy in triangle index order**: triangle 0 takes colour
/// 0, and each triangle after it takes the lowest colour none of its already
/// coloured neighbours has. That order is stated because it has to be -- two
/// peers that coloured differently would thread differently, and a game that
/// hashed a per-colour partial result would desync. Index order is the order
/// everything else in this crate iterates in, and it needs nothing but the
/// mesh.
///
/// Three colours is the usual answer for ground triangulated in strips and four
/// is the worst case.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NavColours {
    of: Vec<u8>,
    order: Vec<NavTriRef>,
    starts: [u32; MAX_COLOURS + 1],
}

impl NavColours {
    /// What colour a triangle is, or [`None`] if it is not one of this mesh's.
    #[must_use]
    #[inline]
    pub fn colour_of(&self, reference: NavTriRef) -> Option<u8> {
        self.of.get(reference.0 as usize).copied()
    }

    /// The colour of every triangle, indexed by [`NavTriRef`].
    #[must_use]
    #[inline]
    pub fn of(&self) -> &[u8] {
        &self.of
    }

    /// How many colours the surface needs.
    ///
    /// Never more than [`MAX_COLOURS`], and this is the number a caller reports
    /// when it wants to know how many passes a threaded tick takes.
    #[must_use]
    #[inline]
    pub fn count(&self) -> usize {
        (0..MAX_COLOURS)
            .filter(|colour| !self.class(*colour).is_empty())
            .count()
    }

    /// Every triangle of one colour, in triangle order.
    ///
    /// Empty for a colour the surface did not need, and for one past
    /// [`MAX_COLOURS`].
    #[must_use]
    #[inline]
    pub fn class(&self, colour: usize) -> &[NavTriRef] {
        let (Some(&start), Some(&end)) = (self.starts.get(colour), self.starts.get(colour + 1))
        else {
            return &[];
        };
        self.order
            .get(start as usize..end as usize)
            .unwrap_or_default()
    }

    /// The colour classes, in colour order.
    ///
    /// What a threaded tick iterates: one pass per class, the class's triangles
    /// handed out to as many threads as there are, and a barrier between
    /// classes.
    pub fn classes(&self) -> impl Iterator<Item = &[NavTriRef]> {
        (0..MAX_COLOURS)
            .map(|colour| self.class(colour))
            .filter(|class| !class.is_empty())
    }

    /// Colours a mesh's triangles.
    pub(crate) fn build(tris: &[NavTri]) -> Self {
        let mut of = vec![u8::MAX; tris.len()];
        for (index, tri) in tris.iter().enumerate() {
            let mut taken = [false; MAX_COLOURS];
            for neighbour in tri.edges().into_iter().flatten().map(NavTriEdge::next) {
                if let Some(&colour) = of.get(neighbour.0 as usize)
                    && let Some(slot) = taken.get_mut(colour as usize)
                {
                    *slot = true;
                }
            }
            let colour = taken.iter().position(|used| !used).unwrap_or(0);
            if let Some(slot) = of.get_mut(index) {
                *slot = u8::try_from(colour).unwrap_or(0);
            }
        }

        let mut starts = [0u32; MAX_COLOURS + 1];
        for &colour in &of {
            if let Some(slot) = starts.get_mut(colour as usize + 1) {
                *slot += 1;
            }
        }
        for colour in 0..MAX_COLOURS {
            let carried = starts[colour];
            if let Some(slot) = starts.get_mut(colour + 1) {
                *slot += carried;
            }
        }

        let mut filled = starts;
        let mut order = vec![NavTriRef(0); of.len()];
        for (index, &colour) in of.iter().enumerate() {
            let Some(cursor) = filled.get_mut(colour as usize) else {
                continue;
            };
            let slot = *cursor as usize;
            *cursor += 1;
            if let Some(place) = order.get_mut(slot) {
                *place = NavTriRef(u32::try_from(index).unwrap_or(u32::MAX));
            }
        }

        Self { of, order, starts }
    }
}
