//! For now, tests are going to go here, because apparently I can't set a
//! global allocator in a test... who knew!?

#![feature(allocator_api)]

use std::{collections::LinkedList, ptr::null, rc::Rc};

use hashbrown::HashMap;
use lhl::{
    alloc::{
        dlmalloc::DlMalloc,
        manual::Unique,
        tracing::{AllocId, FindPointerMethod, TracingAlloc},
    },
    graph::{gui::AllocGraphNodeLayout, AllocGraph},
};

#[global_allocator]
static GLOBAL: TracingAlloc<DlMalloc> =
    unsafe { TracingAlloc::new(DlMalloc::new(), DlMalloc::new()) };

struct ListNode {
    value: i32,
    next: Option<Box<ListNode>>,
}

impl ListNode {
    fn new(value: i32) -> Self {
        Self { value, next: None }
    }

    fn with_parent(self, value: i32) -> Self {
        Self {
            value,
            next: Some(Box::new(self)),
        }
    }

    fn from_rev_iter(iter: impl IntoIterator<Item = i32>) -> Option<Box<Self>> {
        let mut node = None;
        for n in iter {
            node = Some(Box::new(ListNode {
                value: n,
                next: node,
            }));
        }
        node
    }
}

trait UniquePtr: Sized {
    fn ascribe_meta(&mut self);

    fn with_meta(mut self) -> Self {
        self.ascribe_meta();
        self
    }
}
use rand::{thread_rng, Rng};
use std::cell::RefCell;

struct Node {
    data: i32,
    children: RefCell<Vec<Rc<Node>>>,
}

impl Node {
    fn new(data: i32) -> Rc<Self> {
        Rc::new(Node {
            data,
            children: RefCell::new(vec![]),
        })
    }
}

fn random_graph(node_count: usize, edge_probability: f64) -> Vec<Rc<Node>> {
    let mut rng = thread_rng();
    let mut nodes = Vec::with_capacity(node_count);

    // Create all nodes with unique data
    for i in 0..node_count {
        nodes.push(Node::new(i as i32));
    }

    // Randomly add edges between nodes
    for i in 0..node_count {
        for j in 0..node_count {
            if i != j && rng.gen_bool(edge_probability) {
                let parent = &nodes[i];
                let child = nodes[j].clone();
                parent.children.borrow_mut().push(child);
            }
        }
    }

    nodes
}

fn main() {
    let graph = random_graph(80, 0.1); // Create a graph with 10 nodes and 30% chance of an edge between any two nodes

    // For demonstration, let's print the children count for each node
    for (i, node) in graph.iter().enumerate() {
        println!("Node {} has {} children.", i, node.children.borrow().len());
    }
    let n = Box::new(4);

    let buf = vec![
        vec![Box::new(42); 2],
        vec![Box::new(42); 3],
        vec![Box::new(42); 1],
        vec![Box::new(42); 10],
        vec![Box::new(42); 6],
    ];
    // let mut buf = Box::new(SelfReferencingFoo { this: Vec::new() });
    // buf.this = vec![buf.as_ref() as *const SelfReferencingFoo; 10];
    // let buf = std::collections::HashMap::<i32, i32>::from_iter((0..10).zip(0..));
    let graph = AllocGraph::from_sized(&GLOBAL, &buf, FindPointerMethod::AlignedHeapOnly)
        .write_to_file("test.json");
}
