use std::{cmp, f32::consts::PI};

use eframe::CreationContext;
use egui::{
    epaint::{CircleShape, RectShape, TextShape},
    Color32, FontFamily, FontId, LayerId, Painter, Pos2, Rect, Rounding, Stroke, Widget,
};
use egui_graphs::{
    DefaultEdgeShape, DefaultNodeShape, DisplayNode, Graph, GraphView, NodeProps,
    SettingsInteraction, SettingsNavigation, SettingsStyle,
};
use hashbrown::{HashMap, HashSet};
use petgraph::{graph::NodeIndex, stable_graph::StableGraph, Directed};

use crate::{
    alloc::tracing::{AllocId, HexDump},
    graph::{AllocGraph, AllocNode},
};

#[derive(Debug, Clone, PartialEq)]

pub struct AllocNodePayload {
    alloc_id: AllocId,
    hex_dump: HexDump,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AllocNodeShape {
    loc: Pos2,
    payload: AllocNodePayload,
}

impl AllocNodeShape {
    const PADDING: f32 = 20.;

    pub(crate) fn radius_of(alloc_id: &AllocId) -> f32 {
        // pi * r**2 = area
        // (area / pi).sqrt() = r
        let area = alloc_id.size as f32 * 32.;
        (area / PI).sqrt().sqrt() * 8.
    }

    pub(crate) fn width_including_padding_of(alloc_id: &AllocId) -> f32 {
        Self::radius_of(alloc_id) * 2. + Self::PADDING * 2.
    }

    fn radius(&self) -> f32 {
        Self::radius_of(&self.payload.alloc_id)
    }
}

impl From<NodeProps<AllocNodePayload>> for AllocNodeShape {
    fn from(props: NodeProps<AllocNodePayload>) -> Self {
        Self {
            loc: props.location,
            payload: props.payload,
        }
    }
}

fn closest_point_on_circle(center: Pos2, radius: f32, dir: egui::Vec2) -> Pos2 {
    center + dir.normalized() * radius
}

fn is_inside_circle(center: Pos2, radius: f32, pos: Pos2) -> bool {
    let dir = pos - center;
    dir.length() <= radius
}

impl DisplayNode<AllocNodePayload, (), Directed, u32> for AllocNodeShape {
    fn is_inside(&self, pos: Pos2) -> bool {
        is_inside_circle(self.loc, self.radius(), pos)
    }

    fn closest_boundary_point(&self, dir: egui::Vec2) -> Pos2 {
        closest_point_on_circle(self.loc, self.radius(), dir)
    }

    fn shapes(&mut self, ctx: &egui_graphs::DrawContext) -> Vec<egui::Shape> {
        let mut res = vec![];

        let circle_center = ctx.meta.canvas_to_screen_pos(self.loc);
        let circle_radius = ctx.meta.canvas_to_screen_size(self.radius());
        let circle_shape = CircleShape {
            center: circle_center,
            radius: circle_radius,
            fill: Color32::from_rgb(255 - cmp::min(self.payload.alloc_id.size, 255) as u8, 200, cmp::min(255, self.payload.alloc_id.size) as u8),
            stroke: Stroke {
                width: 3.,
                color: Color32::BLACK,
            },
        };
        res.push(circle_shape.into());

        let galley = ctx.ctx.fonts(|f| {
            f.layout_no_wrap(
                format!("{}", self.payload.alloc_id.size),
                FontId::new(circle_radius, FontFamily::Monospace),
                Color32::BLACK,
            )
        });

        let label_pos = Pos2::new(
            circle_center.x - galley.size().x / 2.,
            circle_center.y - circle_radius + galley.size().y / 2.,
        );
        let label_shape = TextShape::new(label_pos, galley, Color32::BLACK);
        res.push(label_shape.into());

        res
    }

    fn update(&mut self, state: &NodeProps<AllocNodePayload>) {
        self.loc = state.location;
    }
}

type HeapGraph = Graph<AllocNodePayload, (), Directed, u32, AllocNodeShape, DefaultEdgeShape>;

type HeapGraphView<'a> =
    GraphView<'a, AllocNodePayload, (), Directed, u32, AllocNodeShape, DefaultEdgeShape>;

pub struct HeapViewApp {
    graph: HeapGraph,
    /// Page base address -> TL location on screen
    pages: HashMap<usize, f32>,
}

const PAGE_SIZE: usize = 4096;

const NODE_SIZE: f32 = 10.;

fn page_size_px(node_size: f32) -> f32 {
    (PAGE_SIZE as f32 * NODE_SIZE).sqrt()
}

pub enum AllocGraphNodeLayout {
    /// A random layout, scaled by the provided scalar
    Random(f32),
    /// A layout optimised for tree-like structures, the scalar here is for the
    /// y-axis
    Tree(f32),
}

impl HeapViewApp {
    pub fn new(
        cc: &CreationContext<'_>,
        alloc_graph: AllocGraph,
        layout: AllocGraphNodeLayout,
    ) -> Self {
        cc.egui_ctx.style_mut(|style| style.visuals.widgets.inactive.fg_stroke = Stroke::new(2f32, Color32::from_rgb(80, 80, 80)));

        let mut graph = StableGraph::<AllocNodePayload, ()>::new();

        let mut nodes_by_page: HashMap<usize, HashSet<NodeIndex<u32>>> = HashMap::new();
        fn page_base_of<T>(addr: *const T) -> usize {
            addr as usize & !(PAGE_SIZE - 1)
        }

        let mut nodes: HashMap<AllocId, NodeIndex<u32>> = HashMap::new();
        for (&k, v) in alloc_graph.graph.iter() {
            nodes.insert(
                k,
                graph.add_node(AllocNodePayload {
                    alloc_id: k,
                    hex_dump: v.hex_dump.clone(),
                }),
            );
        }
        for (&alloc_id, &node) in nodes.iter() {
            if let Some(set) = nodes_by_page.get_mut(&page_base_of(alloc_id.ptr)) {
                set.insert(node);
            } else {
                nodes_by_page.insert(page_base_of(alloc_id.ptr), HashSet::from_iter([node]));
            }

            for edge in alloc_graph
                .graph
                .get(&alloc_id)
                .expect("added from this graph")
                .children
                .iter()
            {
                let child_node = nodes
                    .get(edge.alloc_id())
                    .expect("all children exist as a parent somewhere in this graph");
                graph.add_edge(node, *child_node, ());
            }
        }

        let mut pages = Vec::from_iter(nodes_by_page.keys().copied());
        pages.sort();
        let pages = HashMap::from_iter(
            pages
                .iter()
                .copied()
                .enumerate()
                .map(|(i, val)| (val, dbg!(i as f32 * page_size_px(NODE_SIZE)))),
        );

        let mut graph = HeapGraph::from(&graph);

        match layout {
            AllocGraphNodeLayout::Random(scalar) => {
                for (_, node_index) in nodes.iter() {
                    let node = graph.node_mut(*node_index).unwrap();
                    node.set_location(node.location() * scalar);
                }
            }
            AllocGraphNodeLayout::Tree(scalar) => {
                // TODO: handle cycles in alloc_graph.roots()
                let mut levels: Vec<Vec<AllocId>> = Vec::new();
                let mut unvisited =
                    Vec::from_iter(alloc_graph.roots().into_iter().map(|id| (id, 0)));
                let mut visited: HashSet<AllocId> = HashSet::new();
                while let Some((alloc_id, level)) = unvisited.pop() {
                    if visited.contains(&alloc_id) {
                        continue;
                    }
                    // This means that children are in blocks, so we shouldn't (in the
                    // case of tree-like structures) get interesecting lines
                    while levels.len() <= level {
                        levels.push(Vec::new());
                    }
                    levels[level].push(alloc_id);
                    visited.insert(alloc_id);

                    let children = alloc_graph.graph[&alloc_id]
                        .children
                        .iter()
                        .map(|e| e.alloc_id());

                    unvisited.extend(children.map(|&id| (id, level + 1)));
                }

                let tree_x = 0f32;
                for (level, alloc_ids) in levels.iter().enumerate() {
                    // first pass get the required width of the whole row
                    let mut required_width = 0f32;
                    for alloc_id in alloc_ids {
                        required_width += AllocNodeShape::width_including_padding_of(alloc_id);
                    }
                    // second path place everything
                    let mut x = tree_x - required_width / 2.
                        + AllocNodeShape::width_including_padding_of(
                            alloc_ids
                                .first()
                                .expect("entered for loop, must not be empty"),
                        ) / 2.;
                    for alloc_id in alloc_ids {
                        let node = graph
                            .node_mut(nodes[alloc_id])
                            .expect("all nodes in `nodes` should exist in the graph");
                        node.set_location(Pos2::new(x, level as f32 * 50. * scalar));
                        x += AllocNodeShape::width_including_padding_of(alloc_id);
                    }
                }
            }
        }

        Self { graph, pages }
    }
}

impl eframe::App for HeapViewApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame {
                inner_margin: egui::style::Margin {
                    left: 10.,
                    right: 10.,
                    top: 10.,
                    bottom: 10.,
                },
                outer_margin: egui::style::Margin::default(),
                rounding: egui::Rounding {
                    nw: 1.0,
                    ne: 1.0,
                    sw: 1.0,
                    se: 1.0,
                },
                shadow: eframe::epaint::Shadow::NONE,
                fill: Color32::WHITE,
                stroke: egui::Stroke::new(2.0, Color32::GOLD),
            })
            .show(ctx, |ui| {
                ui.add(
                    &mut HeapGraphView::new(&mut self.graph)
                        .with_interactions(
                            &SettingsInteraction::new()
                                .with_dragging_enabled(true)
                                .with_edge_clicking_enabled(true),
                        )
                        .with_navigations(
                            &SettingsNavigation::new()
                                .with_zoom_and_pan_enabled(true)
                                .with_fit_to_screen_enabled(false),
                        ),
                );
            });
    }
}

pub fn display(graph: AllocGraph, layout: AllocGraphNodeLayout) {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Heap View",
        native_options,
        Box::new(|cc| Box::new(HeapViewApp::new(cc, graph, layout))),
    )
    .unwrap();
}
