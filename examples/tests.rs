//! For now, tests are going to go here, because apparently I can't set a
//! global allocator in a test... who knew!?

#![feature(allocator_api)]

use egui::Link;
use egui_graphs::Graph;
use hashbrown::HashMap;
use lhl::{
    alloc::{
        dlmalloc::DlMalloc,
        manual::Unique,
        tracing::{AllocId, FindPointerMethod, TracingAlloc},
    },
    graph::{gui::AllocGraphNodeLayout, AllocGraph},
};
use std::{any::Any, mem::ManuallyDrop, ptr::null_mut};
use std::{collections::LinkedList, marker::PhantomData, mem::size_of, ptr::null, rc::Rc};

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

    for i in 0..node_count {
        nodes.push(Node::new(i as i32));
    }

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

trait Erasable {
    fn dyn_size() -> usize;
}

impl<T> Erasable for T
where
    T: Sized,
{
    fn dyn_size() -> usize {
        size_of::<Self>()
    }
}

macro_rules! erased_vec {
    ($value:expr; $count:expr) => {{
        use std::any::Any;
        let result: Vec<Box<dyn Any>> = vec![Box::new($value); $count];
    }};
    ($($value:expr),*$(,)?) => {{
        use std::any::Any;
        let result: Vec<Box<dyn Any>> = vec![$(Box::new($value)),*];
        result
    }}
}

type Erase<T> = Box<(dyn Any, PhantomData<T>)>;

struct GraphNode<T>(T, *mut GraphNode<T>);

impl<T> GraphNode<T> {
    fn new(value: T, next: *mut Self) -> *mut Self {
        ManuallyDrop::new(Box::new(Self(value, next))).as_mut()
    }
}

fn main() {
    // let dst = random_graph(200, 0.1); 

    // let mut a = GraphNode::new(42, null_mut());
    // let mut b = GraphNode::new(42, a);
    // let mut c = GraphNode::new(42, b);
    // let mut d = GraphNode::new(42, c);
    // let mut e = GraphNode::new(42, d);
    // unsafe {&mut *a}.1 = e;

    let dst = LinkedList::from_iter(erased_vec![1, 2, 3, 4, 5]);

    // let dst = Box::new(4);

    // let dst: Vec<Box<dyn Sized>> = boxed_vec![
    //     boxed_vec![42; 2],
    //     boxed_vec![42; 3],
    //     boxed_vec![42; 1],
    //     boxed_vec![42; 10],
    //     boxed_vec![42; 6]
    // ];
    // let dst = erased_vec![
    //     erased_vec![42, 42, 42, 42, 42],
    //     erased_vec![1, 2, 3],
    //     erased_vec![1, 2],
    //     erased_vec![1, 2, 3, 4, 5, 6, 7 ,8],
    //     erased_vec![1, 2, 3, 4, 5]
    // ];
    let graph = AllocGraph::from_sized(&GLOBAL, &dst, FindPointerMethod::AlignedHeapOnly)
        .write_to_file("test.json");
    // let mut buf = Box::new(SelfReferencingFoo { this: Vec::new() });
    // buf.this = vec![buf.as_ref() as *const SelfReferencingFoo; 10];
    // let buf = std::collections::HashMap::<i32, i32>::from_iter((0..10).zip(0..));
}
