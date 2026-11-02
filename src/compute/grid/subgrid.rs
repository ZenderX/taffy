//! Subgrids (<https://drafts.csswg.org/css-grid-2/#subgrids>): a grid item whose
//! `grid-template-columns` or `grid-template-rows` is `subgrid` lays its items out on the
//! tracks it spans in its parent grid.
//!
//! The parent sizes its columns first, measuring a subgrid as an independent grid (with implicit
//! tracks in a subgridded axis). It then hands the sized columns down to its column subgrids while
//! it measures them for row sizing, and both axes' tracks to its final layout of each subgrid,
//! which uses them as fixed explicit tracks. So a subgrid's own items do not take part in the
//! parent's track sizing beyond the subgrid's own contribution.
use super::types::{GridItem, GridTrack, NamedLineResolver};
use super::OriginZeroLine;
use crate::geometry::{AbsoluteAxis, Line};
use crate::style::{CheapCloneStr, LengthPercentage, MaxTrackSizingFunction, MinTrackSizingFunction};
use crate::sys::{String, Vec};
use crate::tree::NodeId;

/// The parent tracks a subgrid spans in one axis
#[derive(Debug, Clone)]
pub(super) struct SubgridTracks {
    /// The start and end of each spanned track, measured from the start edge of the subgrid's
    /// grid area in the parent's inline (or block) direction
    pub tracks: Vec<Line<f32>>,
    /// The names of the spanned parent lines, by one-indexed subgrid line
    pub line_names: Vec<(u16, String)>,
}

impl SubgridTracks {
    /// The tracks between the passed track vector indexes of an aligned parent axis, whose grid
    /// area runs from `area.start` to `area.end` (physical offsets)
    pub(super) fn spanned<S: CheapCloneStr>(
        tracks: &[GridTrack],
        indexes: Line<u16>,
        area: Line<f32>,
        is_reversed: bool,
        name_resolver: &NamedLineResolver<S>,
        axis: AbsoluteAxis,
        span: Line<OriginZeroLine>,
    ) -> Self {
        let tracks = (indexes.start as usize + 1..indexes.end as usize)
            .step_by(2)
            .map(|index| {
                let track = &tracks[index];
                let (start, end) = (track.offset, track.offset + track.base_size);
                if is_reversed {
                    Line { start: area.end - end, end: area.end - start }
                } else {
                    Line { start: start - area.start, end: end - area.start }
                }
            })
            .collect();
        Self { tracks, line_names: name_resolver.line_names_in_span(axis, span) }
    }

    /// The tracks between the passed track vector indexes of a sized parent axis whose tracks are
    /// not aligned yet, placed one after the other from their base sizes
    fn sized<S: CheapCloneStr>(
        tracks: &[GridTrack],
        indexes: Line<u16>,
        name_resolver: &NamedLineResolver<S>,
        axis: AbsoluteAxis,
        span: Line<OriginZeroLine>,
    ) -> Self {
        let mut offset = 0.0;
        let mut spanned = Vec::new();
        for (index, track) in tracks.iter().enumerate().take(indexes.end as usize).skip(indexes.start as usize + 1) {
            // Odd indexes are tracks, even ones gutters
            if index % 2 == 1 {
                spanned.push(Line { start: offset, end: offset + track.base_size });
            }
            offset += track.base_size;
        }
        Self { tracks: spanned, line_names: name_resolver.line_names_in_span(axis, span) }
    }

    /// Replace the subgrid's explicit tracks, which start at track vector index
    /// `2 * first_explicit + 1`, and the gutters between them by fixed tracks the size of the
    /// spanned parent tracks and the space between them. The subgrid's content box starts
    /// `content_start` after its grid area's start edge and is `content_size` long; each track is
    /// cut to it, so the tracks at the edges lose the subgrid's margin, border and padding.
    pub(super) fn apply(&self, tracks: &mut [GridTrack], first_explicit: u16, content_start: f32, content_size: f32) {
        let clamp = |offset: f32| (offset - content_start).clamp(0.0, content_size.max(0.0));
        let lines: Vec<Line<f32>> =
            self.tracks.iter().map(|track| Line { start: clamp(track.start), end: clamp(track.end) }).collect();
        let first = 2 * first_explicit as usize + 1;
        for (i, line) in lines.iter().enumerate() {
            let size = line.end - line.start;
            tracks[first + 2 * i] =
                GridTrack::new(MinTrackSizingFunction::length(size), MaxTrackSizingFunction::length(size));
        }
        for (i, pair) in lines.windows(2).enumerate() {
            tracks[first + 2 * i + 1] = GridTrack::gutter(LengthPercentage::length(pair[1].start - pair[0].end));
        }
    }
}

/// The tracks a parent grid hands to one subgrid item
pub(super) struct ParentTracks {
    /// The subgrid
    pub node: NodeId,
    /// Its spanned columns, if it is subgridded in the horizontal axis
    pub columns: Option<SubgridTracks>,
    /// Its spanned rows, if it is subgridded in the vertical axis
    pub rows: Option<SubgridTracks>,
}

/// The sized (not yet aligned) columns of each column subgrid among `items`, for measuring them
/// while the parent sizes its rows
pub(super) fn sized_columns<S: CheapCloneStr>(
    items: &[GridItem],
    columns: &[GridTrack],
    name_resolver: &NamedLineResolver<S>,
) -> Vec<ParentTracks> {
    items
        .iter()
        .filter(|item| item.is_subgrid.horizontal)
        .map(|item| ParentTracks {
            node: item.node,
            columns: Some(SubgridTracks::sized(
                columns,
                item.column_indexes,
                name_resolver,
                AbsoluteAxis::Horizontal,
                item.column,
            )),
            rows: None,
        })
        .collect()
}

#[cfg(feature = "std")]
std::thread_local! {
    /// The parent tracks of the subgrids being laid out, innermost last
    static PARENT_TRACKS: core::cell::RefCell<Vec<ParentTracks>> = const { core::cell::RefCell::new(Vec::new()) };
}

/// Pops the entries pushed by [`with_parent_tracks`], also on unwind
#[cfg(feature = "std")]
struct PopOnDrop(usize);

#[cfg(feature = "std")]
impl Drop for PopOnDrop {
    fn drop(&mut self) {
        PARENT_TRACKS.with(|stack| {
            let mut stack = stack.borrow_mut();
            let len = stack.len() - self.0;
            stack.truncate(len);
        });
    }
}

/// Run `layout` (the parent's layout or measurement of its subgrids) with the passed tracks
/// handed down to each subgrid
pub(super) fn with_parent_tracks<R>(entries: Vec<ParentTracks>, layout: impl FnOnce() -> R) -> R {
    if entries.is_empty() {
        return layout();
    }
    #[cfg(feature = "std")]
    {
        let _pop = PopOnDrop(entries.len());
        PARENT_TRACKS.with(|stack| stack.borrow_mut().extend(entries));
        layout()
    }
    // Without `std` there is no thread-local storage, so subgrids lay out as independent grids
    #[cfg(not(feature = "std"))]
    {
        let _ = entries;
        layout()
    }
}

/// The tracks the parent grid handed to subgrid `node` in the passed axis, if any
pub(super) fn parent_tracks(node: NodeId, axis: AbsoluteAxis) -> Option<SubgridTracks> {
    #[cfg(feature = "std")]
    {
        PARENT_TRACKS.with(|stack| {
            let stack = stack.borrow();
            let entry = stack.iter().rev().find(|entry| entry.node == node)?;
            match axis {
                AbsoluteAxis::Horizontal => entry.columns.clone(),
                AbsoluteAxis::Vertical => entry.rows.clone(),
            }
        })
    }
    #[cfg(not(feature = "std"))]
    {
        let _ = (node, axis);
        None
    }
}

/// Whether a parent grid is laying out or measuring subgrid `node` with tracks handed down to it.
/// Such a layout must not be answered from the cache: an entry with the same inputs may have been
/// computed without the tracks, or with different ones.
pub(crate) fn has_parent_tracks(node: NodeId) -> bool {
    #[cfg(feature = "std")]
    {
        PARENT_TRACKS.with(|stack| stack.borrow().iter().any(|entry| entry.node == node))
    }
    #[cfg(not(feature = "std"))]
    {
        let _ = node;
        false
    }
}
