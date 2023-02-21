use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::fmt::Write;
use std::hash::{Hash, Hasher};

use roaring::RoaringBitmap;

use super::cheapest_paths::Path;
use super::{Edge, EdgeDetails, RankingRuleGraph, RankingRuleGraphTrait};
use crate::new::QueryNode;

#[derive(Debug)]
pub struct PathsMap<V> {
    nodes: Vec<(u32, PathsMap<V>)>,
    value: Option<V>,
}
impl<V> Default for PathsMap<V> {
    fn default() -> Self {
        Self { nodes: vec![], value: None }
    }
}

impl PathsMap<u64> {
    pub fn from_paths(paths: &[Path]) -> Self {
        let mut result = Self::default();
        for p in paths {
            result.add_path(p);
        }
        result
    }
    pub fn add_path(&mut self, path: &Path) {
        self.insert(path.edges.iter().copied(), path.cost);
    }
}
impl<V> PathsMap<V> {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.value.is_none()
    }

    pub fn insert(&mut self, mut edges: impl Iterator<Item = u32>, value: V) {
        match edges.next() {
            None => {
                self.value = Some(value);
            }
            Some(first_edge) => {
                // comment
                for (edge, next_node) in &mut self.nodes {
                    if edge == &first_edge {
                        return next_node.insert(edges, value);
                    }
                }
                let mut rest = PathsMap::default();
                rest.insert(edges, value);
                self.nodes.push((first_edge, rest));
            }
        }
    }
    fn remove_first_rec(&mut self, cur: &mut Vec<u32>) -> (bool, V) {
        let Some((first_edge, rest)) = self.nodes.first_mut() else { 
            // The PathsMap has to be correct by construction here, otherwise
            // the unwrap() will crash
            return (true, self.value.take().unwrap()) 
        };
        cur.push(*first_edge);
        let (rest_is_empty, value) = rest.remove_first_rec(cur);
        if rest_is_empty {
            self.nodes.remove(0);
            (self.nodes.is_empty(), value)
        } else {
            (false, value)
        }
    }
    pub fn remove_first(&mut self) -> Option<(Vec<u32>, V)> {
        if self.is_empty() {
            return None;
        }

        let mut result = vec![];
        let (_, value) = self.remove_first_rec(&mut result);
        Some((result, value))
    }
    pub fn iterate_rec(&self, cur: &mut Vec<u32>, visit: &mut impl FnMut(&Vec<u32>, &V)) {
        if let Some(value) = &self.value {
            visit(cur, value);
        }
        for (first_edge, rest) in self.nodes.iter() {
            cur.push(*first_edge);
            rest.iterate_rec(cur, visit);
            cur.pop();
        }
    }
    pub fn iterate(&self, mut visit: impl FnMut(&Vec<u32>, &V)) {
        self.iterate_rec(&mut vec![], &mut visit)
    }

    pub fn remove_prefixes<U>(&mut self, prefixes: &PathsMap<U>) {
        prefixes.iterate(|prefix, _v| {
            self.remove_prefix(prefix);
        });
    }
    pub fn remove_edges(&mut self, forbidden_edges: &RoaringBitmap) {
        let mut i = 0;
        while i < self.nodes.len() {
            let should_remove = if forbidden_edges.contains(self.nodes[i].0) {
                true
            } else if !self.nodes[i].1.nodes.is_empty() {
                self.nodes[i].1.remove_edges(forbidden_edges);
                self.nodes[i].1.nodes.is_empty()
            } else {
                false
            };
            if should_remove {
                self.nodes.remove(i);
            } else {
                i += 1;
            }
        }
    }
    pub fn remove_edge(&mut self, forbidden_edge: &u32) {
        let mut i = 0;
        while i < self.nodes.len() {
            let should_remove = if &self.nodes[i].0 == forbidden_edge {
                true
            } else if !self.nodes[i].1.nodes.is_empty() {
                self.nodes[i].1.remove_edge(forbidden_edge);
                self.nodes[i].1.nodes.is_empty()
            } else {
                false
            };
            if should_remove {
                self.nodes.remove(i);
            } else {
                i += 1;
            }
        }
    }
    pub fn remove_prefix(&mut self, forbidden_prefix: &[u32]) {
        let [first_edge, remaining_prefix @ ..] = forbidden_prefix else {
            self.nodes.clear();
            self.value = None;
            return;
        };

        let mut i = 0;
        while i < self.nodes.len() {
            let edge = self.nodes[i].0;
            let should_remove = if edge == *first_edge {
                self.nodes[i].1.remove_prefix(remaining_prefix);
                self.nodes[i].1.nodes.is_empty()
            } else {
                false
            };
            if should_remove {
                self.nodes.remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn edge_indices_after_prefix(&self, prefix: &[u32]) -> Vec<u32> {
        let [first_edge, remaining_prefix @ ..] = prefix else {
            return self.nodes.iter().map(|n| n.0).collect();
        };
        for (edge, rest) in self.nodes.iter() {
            if edge == first_edge {
                return rest.edge_indices_after_prefix(remaining_prefix);
            }
        }
        vec![]
    }

    pub fn contains_prefix_of_path(&self, path: &[u32]) -> bool {
        if self.value.is_some() {
            return true;
        }
        match path {
            [] => false,
            [first_edge, remaining_path @ ..] => {
                for (edge, rest) in self.nodes.iter() {
                    if edge == first_edge {
                        return rest.contains_prefix_of_path(remaining_path);
                    }
                }
                false
            }
        }
    }

    pub fn graphviz<G: RankingRuleGraphTrait>(&self, graph: &RankingRuleGraph<G>) -> String {
        let mut desc = String::new();
        desc.push_str("digraph G {\n");
        self.graphviz_rec(&mut desc, vec![], graph);
        desc.push_str("\n}\n");
        desc
    }
    fn graphviz_rec<G: RankingRuleGraphTrait>(
        &self,
        desc: &mut String,
        path_from: Vec<u64>,
        graph: &RankingRuleGraph<G>,
    ) {
        let id_from = {
            let mut h = DefaultHasher::new();
            path_from.hash(&mut h);
            h.finish()
        };
        for (edge_idx, rest) in self.nodes.iter() {
            let Some(Edge { from_node, to_node, cost, details }) = graph.all_edges[*edge_idx as usize].as_ref() else {
                continue;
            };
            let mut path_to = path_from.clone();
            path_to.push({
                let mut h = DefaultHasher::new();
                edge_idx.hash(&mut h);
                h.finish()
            });
            let id_to = {
                let mut h = DefaultHasher::new();
                path_to.hash(&mut h);
                h.finish()
            };
            writeln!(desc, "{id_to} [label = \"{from_node}→{to_node} [{cost}]\"];").unwrap();
            writeln!(desc, "{id_from} -> {id_to};").unwrap();

            rest.graphviz_rec(desc, path_to, graph);
        }
    }
}

impl<G: RankingRuleGraphTrait> RankingRuleGraph<G> {
    pub fn graphviz_with_path(&self, path: &Path) -> String {
        let mut desc = String::new();
        desc.push_str("digraph G {\nrankdir = LR;\nnode [shape = \"record\"]\n");

        for (node_idx, node) in self.query_graph.nodes.iter().enumerate() {
            if matches!(node, QueryNode::Deleted) {
                continue;
            }
            desc.push_str(&format!("{node_idx} [label = {:?}]", node));
            if node_idx == self.query_graph.root_node as usize {
                desc.push_str("[color = blue]");
            } else if node_idx == self.query_graph.end_node as usize {
                desc.push_str("[color = red]");
            }
            desc.push_str(";\n");
        }

        for (edge_idx, edge) in self.all_edges.iter().enumerate() {
            let Some(edge) = edge else { continue };
            let Edge { from_node, to_node, cost, details } = edge;
            let color = if path.edges.contains(&(edge_idx as u32)) { "red" } else { "green" };
            match &edge.details {
                EdgeDetails::Unconditional => {
                    desc.push_str(&format!(
                        "{from_node} -> {to_node} [label = \"cost {cost}\", color = {color}];\n",
                        cost = edge.cost,
                    ));
                }
                EdgeDetails::Data(details) => {
                    desc.push_str(&format!(
                        "{from_node} -> {to_node} [label = \"cost {cost} {edge_label}\", color = {color}];\n",
                        cost = edge.cost,
                        edge_label = G::graphviz_edge_details_label(details),
                    ));
                }
            }
        }

        desc.push('}');
        desc
    }
}