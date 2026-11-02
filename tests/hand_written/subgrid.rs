//! Subgrids (css-grid-2 §9): a grid item with `grid-template-columns: subgrid` (or rows) lays its
//! items out on the parent tracks it spans, inherits the parent's gutters and line names, and
//! cuts its own margin, border and padding from the tracks at its edges.
#[cfg(feature = "grid")]
mod subgrid {
    use taffy::prelude::*;
    use taffy_test_helpers::{new_test_tree, TestNodeContext};

    type Tree = TaffyTree<TestNodeContext>;

    const PARENT_WIDTH: f32 = 600.0;

    fn leaf(tree: &mut Tree) -> NodeId {
        tree.new_leaf(Style { size: Size { width: auto(), height: length(10.0) }, ..Default::default() }).unwrap()
    }

    fn parent_style(columns: [f32; 3], gap: f32) -> Style {
        Style {
            display: Display::Grid,
            size: Size { width: length(PARENT_WIDTH + 2.0 * gap), height: auto() },
            grid_template_columns: columns.iter().map(|&size| length(size)).collect(),
            gap: Size { width: length(gap), height: zero() },
            ..Default::default()
        }
    }

    fn column_subgrid(padding_left: f32) -> Style {
        Style {
            display: Display::Grid,
            grid_template_columns_subgrid: true,
            grid_column: Line { start: span(3), end: GridPlacement::Auto },
            padding: Rect { left: length(padding_left), right: zero(), top: zero(), bottom: zero() },
            ..Default::default()
        }
    }

    /// A parent grid with the passed column tracks holding one column subgrid that spans all three
    /// and holds three auto-placed items; returns the tree, the parent, the subgrid and the items.
    fn column_layout(columns: [f32; 3], gap: f32, subgrid_style: Style) -> (Tree, NodeId, NodeId, Vec<NodeId>) {
        let mut tree = new_test_tree();
        let items: Vec<NodeId> = (0..3).map(|_| leaf(&mut tree)).collect();
        let subgrid = tree.new_with_children(subgrid_style, &items).unwrap();
        let parent = tree.new_with_children(parent_style(columns, gap), &[subgrid]).unwrap();
        tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
        (tree, parent, subgrid, items)
    }

    fn x_and_width(tree: &Tree, items: &[NodeId]) -> Vec<(f32, f32)> {
        items
            .iter()
            .map(|&item| (tree.layout(item).unwrap().location.x, tree.layout(item).unwrap().size.width))
            .collect()
    }

    /// css-grid-2 §9.1: the subgrid's columns are the parent's `100px 200px 300px`, so its items
    /// start at the parent's column lines 0, 100 and 300 although the subgrid sets no tracks, and
    /// share one row: the parent sizes its row from the subgrid measured on those columns.
    #[test]
    fn subgrid_items_sit_on_the_parent_column_tracks() {
        let (tree, parent, subgrid, items) = column_layout([100.0, 200.0, 300.0], 0.0, column_subgrid(0.0));
        assert_eq!(x_and_width(&tree, &items), vec![(0.0, 100.0), (100.0, 200.0), (300.0, 300.0)]);
        assert_eq!(tree.layout(subgrid).unwrap().size, Size { width: 600.0, height: 10.0 }, "one row of items");
        assert_eq!(tree.layout(parent).unwrap().size.height, 10.0, "the parent's row holds that one row");
    }

    /// A plain nested grid with the same placement lays its three items out on its own single
    /// implicit column, one under the other: the parent's tracks reach it only through `subgrid`.
    #[test]
    fn a_nested_grid_without_subgrid_has_its_own_tracks() {
        let style = Style { grid_template_columns_subgrid: false, ..column_subgrid(0.0) };
        let (tree, _, _, items) = column_layout([100.0, 200.0, 300.0], 0.0, style);
        assert_eq!(x_and_width(&tree, &items), vec![(0.0, 600.0), (0.0, 600.0), (0.0, 600.0)]);
    }

    /// css-grid-2 §9.3: the parent's 10px gutters separate the subgrid's tracks too.
    #[test]
    fn the_parent_gutters_separate_the_subgrid_tracks() {
        let (tree, _, _, items) = column_layout([100.0, 200.0, 300.0], 10.0, column_subgrid(0.0));
        assert_eq!(x_and_width(&tree, &items), vec![(0.0, 100.0), (110.0, 200.0), (320.0, 300.0)]);
    }

    /// css-grid-2 §9.5: the subgrid's padding is cut from the track at its edge only, so its
    /// first item starts after the padding and the others stay on the parent's lines.
    #[test]
    fn subgrid_padding_shrinks_only_the_edge_track() {
        let (tree, _, _, items) = column_layout([100.0, 200.0, 300.0], 0.0, column_subgrid(20.0));
        assert_eq!(x_and_width(&tree, &items), vec![(20.0, 80.0), (100.0, 200.0), (300.0, 300.0)]);
    }

    /// css-grid-2 §9.4: an item of the subgrid placed at a parent line name (`b`, the parent's
    /// second column line) starts on that line.
    #[test]
    fn subgrid_items_resolve_the_parent_line_names() {
        let mut tree = new_test_tree();
        let named = tree
            .new_leaf(Style {
                size: Size { width: auto(), height: length(10.0) },
                grid_column: Line { start: GridPlacement::NamedLine("b".into(), 0), end: GridPlacement::Auto },
                ..Default::default()
            })
            .unwrap();
        let subgrid = tree.new_with_children(column_subgrid(0.0), &[named]).unwrap();
        let parent = tree
            .new_with_children(
                Style {
                    grid_template_column_names: vec![vec!["a".into()], vec!["b".into()], vec![], vec![]],
                    ..parent_style([100.0, 200.0, 300.0], 0.0)
                },
                &[subgrid],
            )
            .unwrap();
        tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
        assert_eq!(x_and_width(&tree, &[named]), vec![(100.0, 200.0)]);
    }

    /// css-grid-2 §9.1 in the block axis: a row subgrid spanning the parent's `50px 80px` rows
    /// puts its two items at y = 0 and 50.
    #[test]
    fn subgrid_items_sit_on_the_parent_row_tracks() {
        let mut tree = new_test_tree();
        let items: Vec<NodeId> = (0..2)
            .map(|row| {
                tree.new_leaf(Style {
                    size: Size { width: length(10.0), height: auto() },
                    grid_row: line(row + 1),
                    ..Default::default()
                })
                .unwrap()
            })
            .collect();
        let subgrid = tree
            .new_with_children(
                Style {
                    display: Display::Grid,
                    grid_template_rows_subgrid: true,
                    grid_row: Line { start: span(2), end: GridPlacement::Auto },
                    ..Default::default()
                },
                &items,
            )
            .unwrap();
        let parent = tree
            .new_with_children(
                Style {
                    display: Display::Grid,
                    grid_template_rows: vec![length(50.0), length(80.0)],
                    ..Default::default()
                },
                &[subgrid],
            )
            .unwrap();
        tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
        let rows: Vec<(f32, f32)> = items
            .iter()
            .map(|&item| (tree.layout(item).unwrap().location.y, tree.layout(item).unwrap().size.height))
            .collect();
        assert_eq!(rows, vec![(0.0, 50.0), (50.0, 80.0)]);
    }

    /// The subgrid follows a change of the parent's tracks that keeps its own size (600px):
    /// its cached layout from the earlier tracks must not be reused.
    #[test]
    fn subgrid_follows_parent_tracks_that_keep_its_size() {
        let (mut tree, parent, _, items) = column_layout([100.0, 200.0, 300.0], 0.0, column_subgrid(0.0));
        tree.set_style(parent, parent_style([300.0, 200.0, 100.0], 0.0)).unwrap();
        tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
        assert_eq!(x_and_width(&tree, &items), vec![(0.0, 300.0), (300.0, 200.0), (500.0, 100.0)]);
    }
}
