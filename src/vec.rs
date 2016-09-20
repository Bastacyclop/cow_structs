use std::sync::Arc;
use std::mem;
use arrayvec::ArrayVec;

pub const NODE_SIZE: usize = 32;
pub const SHIFT: usize = 5;
pub const MASK: usize = NODE_SIZE - 1;

#[derive(Clone, Debug)]
enum Node<V> {
    Internal(Arc<InternalNode<V>>),
    External(Arc<ExternalNode<V>>),
    Empty,
}

type InternalNode<V> = ArrayVec<[Node<V>; NODE_SIZE]>;
type ExternalNode<V> = ArrayVec<[V; NODE_SIZE]>;

#[derive(Debug, Clone)]
pub struct CowVec<V> {
    root: Node<V>,
    depth: usize,
    tail: Arc<ExternalNode<V>>,
    len: usize,
}

impl<V: Clone> CowVec<V> {
    pub fn new() -> Self {
        CowVec {
            root: Node::Empty,
            depth: 0,
            tail: Arc::new(ExternalNode::new()),
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn push(&mut self, value: V) {
        if self.tail.len() < NODE_SIZE {
            Arc::make_mut(&mut self.tail).push(value);
            self.len += 1;
            return;
        }

        let new_tail = new_external_node(value);
        let old_tail = Node::External(mem::replace(&mut self.tail, new_tail));

        // special case where the tail becomes the root
        if self.len == NODE_SIZE {
            self.root = old_tail;
            self.len += 1;
            return;
        }

        if let &mut Node::Internal(ref mut r) = &mut self.root {
            if r.len() < NODE_SIZE {
                Self::push_external(Arc::make_mut(r), self.depth, old_tail);
                self.len += 1;
                return;
            }
        }

        let old_root = mem::replace(&mut self.root, new_internal_node());
        let new_root = self.root.make_internal_mut();
        new_root.push(old_root);
        self.depth += 1;
        Self::new_path(new_root, self.depth, old_tail);
        self.len += 1;
    }

    fn push_external(node: &mut InternalNode<V>, depth: usize, ext: Node<V>) {
        if depth == 1 {
            node.push(ext);
        } else {
            if let Some(n) = node.last_mut() {
                return Self::push_external(n.make_internal_mut(), depth - 1, ext);
            }

            Self::new_path(node, depth, ext);
        }
    }

    fn new_path(node: &mut InternalNode<V>, depth: usize, ext: Node<V>) {
        if depth == 1 {
            node.push(ext);
        } else {
            let new_node = new_internal_node();
            node.push(new_node);
            let new_node = node.last_mut().unwrap();
            Self::new_path(new_node.make_internal_mut(), depth - 1, ext);
        }
    }

    pub fn pop(&mut self) -> Option<V> {
        if self.len == 0 { return None; }
        if self.len == 1 || self.tail.len() > 1 {
            self.len -= 1;
            return Arc::make_mut(&mut self.tail).pop();
        }

        // special case where the root becomes the tail
        if (self.len - 1) == NODE_SIZE {
            self.depth = 0;
            self.len -= 1;
            // TODO: clone one or make mut all ?
            let value = self.tail.last().unwrap().clone();
            self.tail = mem::replace(&mut self.root, Node::Empty).to_external();
            return Some(value);
        }

        // TODO: clone one or make mut all ?
        let value = self.tail.last().unwrap().clone();
        let (ext, _) = Self::pop_external(&mut self.root, self.depth);
        self.tail = ext.to_external();
        self.len -= 1;

        let mut root_killer = None;
        if let &mut Node::Internal(ref mut r) = &mut self.root {
            if r.len() == 1 {
                root_killer = Arc::make_mut(r).pop();
            }
        };
        if let Some(rk) = root_killer {
            mem::replace(&mut self.root, rk);
            self.depth -= 1;
        }

        return Some(value);
    }

    fn pop_external(node: &mut Node<V>, depth: usize) -> (Node<V>, bool) {
        // TODO: clean/optimize this up,
        // I think we can determine where the path should be cut in advance
        // to avoid non-terminal recursion.
        let (ext, empty) = {
            let n = node.make_internal_mut();
            let ext = if depth == 1 {
                n.pop().unwrap()
            } else {
                let mut next = n.pop().unwrap();
                let (ext, next_empty) = Self::pop_external(&mut next, depth - 1);
                if !next_empty { n.push(next); }
                ext
            };
            (ext, n.is_empty())
        };

        if empty { *node = Node::Empty; }
        (ext, empty)
    }

    pub fn get_mut(&mut self, index: usize) -> &mut V {
        if index >= self.tail_offset() {
            return &mut Arc::make_mut(&mut self.tail)[index & MASK];
        }

        Self::get_external_mut(&mut self.root, index, self.depth * SHIFT)
    }

    fn tail_offset(&self) -> usize {
        self.len - self.tail.len()
    }

    fn get_external_mut(node: &mut Node<V>,
                        index: usize,
                        shift: usize) -> &mut V {
        match node {
            &mut Node::External(ref mut n) => {
                &mut Arc::make_mut(n)[index & MASK]
            }
            &mut Node::Internal(ref mut n) => {
                let sub_index = (index >> shift) & MASK;
                let next = &mut Arc::make_mut(n)[sub_index];
                Self::get_external_mut(next, index, shift - SHIFT)
            }
            &mut Node::Empty => unreachable!(),
        }
    }

    pub fn get(&mut self, index: usize) -> &V {
        if index >= self.tail_offset() {
            return &self.tail[index & MASK];
        }

        Self::get_external(&self.root, index, self.depth * SHIFT)
    }

    fn get_external(node: &Node<V>, index: usize, shift: usize) -> &V {
        match node {
            &Node::External(ref n) => &n[index & MASK],
            &Node::Internal(ref n) => {
                let sub_index = (index >> shift) & MASK;
                Self::get_external(&n[sub_index], index, shift - SHIFT)
            }
            &Node::Empty => unreachable!(),
        }
    }

    pub fn swap_remove(&mut self, index: usize) -> V {
        // TODO: is there
        let last = self.pop().unwrap();
        if index == self.len {
            last
        } else {
            mem::replace(self.get_mut(index), last)
        }
    }
}

fn new_internal_node<V>() -> Node<V> {
    Node::Internal(Arc::new(InternalNode::new()))
}

fn new_external_node<V>(value: V) -> Arc<ExternalNode<V>> {
    let mut n = ExternalNode::new();
    n.push(value);
    Arc::new(n)
}

impl<V: Clone> Node<V> {
    fn make_internal_mut(&mut self) -> &mut InternalNode<V> {
        match self {
            &mut Node::Internal(ref mut n) => Arc::make_mut(n),
            _ => panic!("expected internal node"),
        }
    }

    fn to_external(self) -> Arc<ExternalNode<V>> {
        match self {
            Node::External(n) => n,
            _ => panic!("expected external node"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn push_pop() {
        let mut v = CowVec::new();
        let n = 3 * NODE_SIZE + NODE_SIZE / 2;
        for i in 0..n {
            v.push(i);
        }
        for i in 0..n {
            assert!(v.pop() == Some(n - 1 - i));
        }
        assert!(v.pop() == None);
    }

    #[test]
    fn push_set_get() {
        let mut v = CowVec::new();
        let n = 3 * NODE_SIZE + NODE_SIZE / 2;

        for i in 0..n {
            v.push(i);
        }
        for i in 0..n {
            if i % 2 == 0 {
                *v.get_mut(i) = i / 2;
            }
        }
        for i in 0..n {
            let expected = if i % 2 == 0 { i / 2 } else { i };
            assert!(v.get(i) == &expected);
        }
    }

    #[test]
    fn swap_remove() {
        let mut v = CowVec::new();
        let h = 3 * NODE_SIZE;
        let n = 2 * h;

        for i in 0..n {
            v.push(i);
        }

        for i in 0..h {
            assert!(v.swap_remove(i) == i);
            assert!(v.get(i) == &(n - 1 - i));
        }
    }
}
