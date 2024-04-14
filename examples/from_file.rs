use lhl::graph::{
    gui::{display, AllocGraphNodeLayout},
    AllocGraph,
};

fn main() {
    // let graph = AllocGraph::from_file(
    //     "/home/oli/Documents/Coding/Rust/Projects/fun/assignment2025cfl5/cw5/cw05/ast.json",
    // )
    // .unwrap();
    let graph = AllocGraph::from_file("pure-c.json").unwrap();
    let mut sizes = graph.kv_pairs().iter().map(|(k, v)| k.size).collect::<Vec<_>>();
    sizes.sort();
    dbg!(&sizes);
    dbg!(sizes[sizes.len() / 2]);
    println!("{graph:#?}");
    display(graph, AllocGraphNodeLayout::Tree(5.));
}
