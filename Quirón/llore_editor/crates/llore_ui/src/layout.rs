//! # Layout Engine
//!
//! Wrapper sobre Taffy para layout Flexbox/Grid.

use std::collections::HashMap;
use taffy::prelude::*;

/// Tamaño en pixels
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Size = Size {
        width: 0.0,
        height: 0.0,
    };

    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

impl From<taffy::Size<f32>> for Size {
    fn from(size: taffy::Size<f32>) -> Self {
        Size {
            width: size.width,
            height: size.height,
        }
    }
}

/// Posición + tamaño de un elemento
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Bounds {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn size(&self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

/// Constraints para layout (espacio disponible)
#[derive(Debug, Clone, Copy)]
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Constraints {
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    pub fn loose(max: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: max.width,
            min_height: 0.0,
            max_height: max.height,
        }
    }

    pub fn unbounded() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }
}

/// Motor de layout basado en Taffy
pub struct LayoutEngine {
    tree: TaffyTree<()>,
    nodes: HashMap<u64, NodeId>,
    next_id: u64,
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self {
            tree: TaffyTree::new(),
            nodes: HashMap::new(),
            next_id: 0,
        }
    }

    /// Crea un nuevo nodo de layout con el estilo dado
    pub fn create_node(&mut self, style: taffy::Style) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let node_id = self.tree.new_leaf(style).expect("Failed to create node");
        self.nodes.insert(id, node_id);
        id
    }

    /// Añade hijos a un nodo
    pub fn set_children(&mut self, parent: u64, children: &[u64]) {
        let parent_id = self.nodes.get(&parent).copied();
        if let Some(parent_id) = parent_id {
            let child_ids: Vec<_> = children
                .iter()
                .filter_map(|c| self.nodes.get(c).copied())
                .collect();
            let _ = self.tree.set_children(parent_id, &child_ids);
        }
    }

    /// Calcula el layout para el nodo root
    pub fn compute(&mut self, root: u64, available: Size) {
        if let Some(root_id) = self.nodes.get(&root) {
            let available_space = taffy::Size {
                width: AvailableSpace::Definite(available.width),
                height: AvailableSpace::Definite(available.height),
            };
            let _ = self.tree.compute_layout(*root_id, available_space);
        }
    }

    /// Obtiene los bounds calculados para un nodo
    pub fn get_bounds(&self, node: u64) -> Bounds {
        if let Some(node_id) = self.nodes.get(&node) {
            let layout = self.tree.layout(*node_id).unwrap();
            Bounds {
                x: layout.location.x,
                y: layout.location.y,
                width: layout.size.width,
                height: layout.size.height,
            }
        } else {
            Bounds::default()
        }
    }

    /// Limpia todos los nodos
    pub fn clear(&mut self) {
        self.tree.clear();
        self.nodes.clear();
        self.next_id = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_basic() {
        let mut engine = LayoutEngine::new();

        let root = engine.create_node(taffy::Style {
            size: taffy::Size {
                width: Dimension::Length(100.0),
                height: Dimension::Length(50.0),
            },
            ..Default::default()
        });

        engine.compute(root, Size::new(800.0, 600.0));
        let bounds = engine.get_bounds(root);

        assert_eq!(bounds.width, 100.0);
        assert_eq!(bounds.height, 50.0);
    }
}
