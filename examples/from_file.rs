use lhl::graph::{
    gui::{display, AllocGraphNodeLayout},
    AllocGraph,
};

fn main() {
    let graph = AllocGraph::from_file("test.json").unwrap();
    display(graph, AllocGraphNodeLayout::Tree);
}
